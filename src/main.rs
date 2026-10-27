mod model;
mod theme;

use std::collections::{BTreeMap, BTreeSet};
use zellij_tile::prelude::*;

use model::{age_short, parse_list, Workspace, WsState};
use theme::{Col, Theme};

const REFRESH_SECS: f64 = 5.0;

#[derive(Clone)]
enum View {
    Tree,
    Menu(usize),
    ConfirmSwitch(usize),
    ConfirmFinish(usize),
}

#[derive(Clone)]
enum Target {
    None,
    Workspace(usize),
    MenuRemove(usize),
    MenuCancel,
    SwitchYes(usize),
    SwitchNo,
    FinishYes(usize),
    FinishNo,
}

struct State {
    cfg: BTreeMap<String, String>,
    theme: Theme,
    granted: bool,
    workspaces: Vec<Workspace>,
    current: Option<String>,
    live: BTreeSet<String>,
    error: Option<String>,
    loaded: bool,
    cols: usize,
    view: View,
    line_targets: Vec<Target>,
    new_btn: Option<(usize, usize)>,
    plugin_id: Option<u32>,
    permission_pending: bool,
    saw_terminal: bool,
    selectable_sent: Option<bool>,
}

impl Default for State {
    fn default() -> Self {
        State {
            cfg: BTreeMap::new(),
            theme: Theme::ansi(),
            granted: false,
            workspaces: Vec::new(),
            current: None,
            live: BTreeSet::new(),
            error: None,
            loaded: false,
            cols: 0,
            view: View::Tree,
            line_targets: Vec::new(),
            new_btn: None,
            plugin_id: None,
            permission_pending: true,
            saw_terminal: false,
            selectable_sent: None,
        }
    }
}

register_plugin!(State);

impl ZellijPlugin for State {
    fn load(&mut self, configuration: BTreeMap<String, String>) {
        self.cfg = configuration;
        let mut t = Theme::ansi();
        t.apply_overrides(&self.cfg);
        self.theme = t;

        self.plugin_id = Some(get_plugin_ids().plugin_id);
        request_permission(&[
            PermissionType::ReadApplicationState,
            PermissionType::ChangeApplicationState,
            PermissionType::RunCommands,
            PermissionType::OpenTerminalsOrPlugins,
        ]);
        subscribe(&[
            EventType::ModeUpdate,
            EventType::PermissionRequestResult,
            EventType::SessionUpdate,
            EventType::PaneUpdate,
            EventType::Mouse,
            EventType::Timer,
            EventType::RunCommandResult,
        ]);
        self.sync_selectable();
        set_timeout(1.0);
    }

    fn update(&mut self, event: Event) -> bool {
        match event {
            Event::ModeUpdate(info) => {
                let mut t = Theme::from_styling(&info.style.colors);
                t.apply_overrides(&self.cfg);
                self.theme = t;
                true
            }
            Event::PermissionRequestResult(status) => {
                self.granted = matches!(status, PermissionStatus::Granted);
                self.permission_pending = false;
                self.sync_selectable();
                if self.granted {
                    self.refresh();
                }
                true
            }
            Event::PaneUpdate(manifest) => {
                self.note_panes(&manifest);
                self.sync_selectable();
                false
            }
            Event::Timer(_) => {
                if self.granted {
                    self.refresh();
                }
                set_timeout(REFRESH_SECS);
                false
            }
            Event::RunCommandResult(exit, stdout, stderr, context) => {
                if context.get("cmd").map(String::as_str) == Some("list") {
                    self.loaded = true;
                    if exit == Some(0) {
                        match parse_list(&stdout) {
                            Ok(list) => {
                                self.workspaces = list;
                                self.error = None;
                            }
                            Err(e) => self.error = Some(format!("parse: {e}")),
                        }
                    } else {
                        let msg = String::from_utf8_lossy(&stderr);
                        let msg = msg.lines().next().unwrap_or("dev list failed");
                        self.error = Some(msg.to_string());
                    }
                    return true;
                }
                false
            }
            Event::SessionUpdate(sessions, _resurrectable) => {
                self.live.clear();
                self.current = None;
                for s in &sessions {
                    self.live.insert(s.name.clone());
                    if s.is_current_session {
                        self.current = Some(s.name.clone());
                    }
                }
                true
            }
            Event::Mouse(mouse) => self.handle_mouse(mouse),
            _ => false,
        }
    }

    fn render(&mut self, rows: usize, cols: usize) {
        self.cols = cols.max(1);
        match self.view.clone() {
            View::Tree => self.render_tree(rows),
            View::Menu(i) => self.render_menu(i),
            View::ConfirmSwitch(i) => self.render_confirm_switch(i),
            View::ConfirmFinish(i) => self.render_confirm_finish(i),
        }
    }
}

impl State {
    fn refresh(&mut self) {
        let mut ctx = BTreeMap::new();
        ctx.insert("cmd".to_string(), "list".to_string());
        run_command(&["dev", "list", "--json"], ctx);
    }

    /// Latch once a manifest shows a terminal (non-plugin) pane sharing this
    /// plugin's tab. See `sync_selectable` for why this gates passivity.
    fn note_panes(&mut self, manifest: &PaneManifest) {
        let Some(pid) = self.plugin_id else { return };
        let my_tab = manifest
            .panes
            .iter()
            .find_map(|(tab, panes)| {
                panes.iter().any(|p| p.is_plugin && p.id == pid).then_some(*tab)
            });
        if let Some(tab) = my_tab {
            let has_terminal = manifest
                .panes
                .get(&tab)
                .is_some_and(|panes| panes.iter().any(|p| !p.is_plugin));
            self.saw_terminal |= has_terminal;
        }
    }

    /// Selectable while the permission prompt is in flight (Zellij ties the y/n
    /// prompt to the requesting pane, and a non-selectable pane can't be
    /// focused to answer it) and until a terminal is known to share the tab.
    /// Then it latches passive: Zellij closes any tab whose only panes are
    /// non-selectable, so going passive before a terminal exists would orphan
    /// the tab — and, if it is the last tab, end the session. Once a terminal
    /// has shared the tab, staying passive lets closing that terminal tear the
    /// tab (and session) down normally instead of the plugin keeping it alive.
    fn desired_selectable(&self) -> bool {
        self.permission_pending || !self.saw_terminal
    }

    fn sync_selectable(&mut self) {
        let want = self.desired_selectable();
        if self.selectable_sent != Some(want) {
            self.selectable_sent = Some(want);
            set_selectable(want);
        }
    }

    /// Display order: workspaces youngest-first, grouped by repo; a repo appears
    /// where its youngest workspace falls. Returns indices into `self.workspaces`.
    fn flat_order(&self) -> Vec<usize> {
        let mut repos: Vec<String> = Vec::new();
        let mut groups: Vec<Vec<usize>> = Vec::new();
        for (i, ws) in self.workspaces.iter().enumerate() {
            match repos.iter().position(|r| r == &ws.repo) {
                Some(p) => groups[p].push(i),
                None => {
                    repos.push(ws.repo.clone());
                    groups.push(vec![i]);
                }
            }
        }
        groups.into_iter().flatten().collect()
    }

    fn is_running(&self, ws: &Workspace) -> bool {
        self.live.contains(&ws.session) || ws.state == WsState::Running
    }

    fn is_current(&self, ws: &Workspace) -> bool {
        if self.cfg.get("demo_current").map(String::as_str) == Some(ws.label.as_str()) {
            return true;
        }
        self.current.as_deref() == Some(ws.session.as_str())
    }

    fn activate(&mut self, i: usize) {
        if i >= self.workspaces.len() {
            return;
        }
        let ws = self.workspaces[i].clone();
        if self.is_current(&ws) {
            return;
        }
        if self.is_running(&ws) {
            switch_session(Some(&ws.session));
        } else {
            open_command_pane_floating(
                CommandToRun::new_with_args("dev", vec!["resume".to_string(), ws.label.clone()]),
                None,
                BTreeMap::new(),
            );
        }
    }

    fn finish(&mut self, i: usize) {
        if i >= self.workspaces.len() {
            return;
        }
        let label = self.workspaces[i].label.clone();
        open_command_pane_floating(
            CommandToRun::new_with_args(
                "dev",
                vec!["finish".to_string(), "--yes".to_string(), label],
            ),
            None,
            BTreeMap::new(),
        );
    }

    fn new_session(&mut self) {
        open_command_pane_floating(
            CommandToRun::new_with_args("dev", vec!["start".to_string()]),
            None,
            BTreeMap::new(),
        );
    }

    fn handle_mouse(&mut self, mouse: Mouse) -> bool {
        match mouse {
            Mouse::LeftClick(line, col) => {
                if line < 0 {
                    return false;
                }
                let line = line as usize;
                if line == 0 {
                    if let Some((a, b)) = self.new_btn {
                        if col >= a && col < b {
                            self.new_session();
                            return false;
                        }
                    }
                }
                match self.line_targets.get(line).cloned() {
                    Some(Target::Workspace(i)) => {
                        if self.workspaces.get(i).map(|w| self.is_current(w)) == Some(true) {
                            return false;
                        }
                        self.view = View::ConfirmSwitch(i);
                        return true;
                    }
                    Some(Target::MenuRemove(i)) => {
                        self.view = View::ConfirmFinish(i);
                        return true;
                    }
                    Some(Target::SwitchYes(i)) => {
                        self.activate(i);
                        self.view = View::Tree;
                        return true;
                    }
                    Some(Target::FinishYes(i)) => {
                        self.finish(i);
                        self.view = View::Tree;
                        return true;
                    }
                    Some(Target::MenuCancel)
                    | Some(Target::SwitchNo)
                    | Some(Target::FinishNo) => {
                        self.view = View::Tree;
                        return true;
                    }
                    _ => {}
                }
                false
            }
            Mouse::RightClick(line, _col) => {
                if line < 0 {
                    return false;
                }
                if let Some(Target::Workspace(i)) = self.line_targets.get(line as usize).cloned() {
                    self.view = View::Menu(i);
                    return true;
                }
                false
            }
            _ => false,
        }
    }

    fn render_tree(&mut self, rows: usize) {
        let cols = self.cols;
        let th = self.theme;
        let mut out: Vec<String> = Vec::new();
        let mut targets: Vec<Target> = Vec::new();

        // Title: left label + right-aligned [+ new], both muted.
        let btn = "[+ new]";
        let label = " DEV SESSIONS";
        let btn_start = cols.saturating_sub(btn.len());
        let shown = trunc(label, btn_start.saturating_sub(1));
        let mut title = styled(th.title, false, &shown);
        for _ in shown.chars().count()..btn_start {
            title.push(' ');
        }
        title.push_str(&styled(th.dim, false, btn));
        out.push(title);
        targets.push(Target::None);
        self.new_btn = Some((btn_start, cols));

        if let Some(err) = &self.error {
            out.push(styled(th.error, false, &trunc(err, cols)));
            targets.push(Target::None);
        }

        let order = self.flat_order();
        if order.is_empty() && self.error.is_none() {
            let msg = if !self.loaded { "loading…" } else { "no workspaces" };
            out.push(styled(th.dim, false, &trunc(msg, cols)));
            targets.push(Target::None);
        }

        let mut last_repo: Option<String> = None;
        for &i in &order {
            let ws = &self.workspaces[i];
            if last_repo.as_deref() != Some(ws.repo.as_str()) {
                out.push(styled(th.dim, false, &trunc(&ws.repo, cols)));
                targets.push(Target::None);
                last_repo = Some(ws.repo.clone());
            }

            let current = self.is_current(ws);
            let running = self.is_running(ws);
            let glyph = if running {
                "●"
            } else if ws.state == WsState::Exited {
                "○"
            } else {
                "·"
            };
            let glyph_col = if current {
                th.accent
            } else if running {
                th.running
            } else {
                th.dim
            };
            let name_col = if current { th.accent } else { th.muted };
            let bg = if current { Some(th.hl_bg) } else { None };

            let age = age_short(ws.created);
            let prefix = 2 + 1 + 1; // indent + glyph + space
            let avail = cols.saturating_sub(prefix + age.chars().count() + 1);
            let name = trunc(&ws.workspace, avail.max(1));
            let pad = cols.saturating_sub(prefix + name.chars().count() + age.chars().count());

            out.push(row(
                bg,
                current,
                &[
                    (glyph_col, "  "),
                    (glyph_col, glyph),
                    (name_col, " "),
                    (name_col, &name),
                ],
                pad,
                (th.dim, &age),
            ));
            targets.push(Target::Workspace(i));
        }

        self.line_targets = targets;
        emit(out, rows);
    }

    fn render_menu(&mut self, i: usize) {
        let th = self.theme;
        let cols = self.cols;
        let label = self.label_of(i);
        let out = vec![
            styled(th.muted, true, &trunc(&label, cols)),
            String::new(),
            styled(th.error, false, &trunc("  remove workspace", cols)),
            styled(th.dim, false, &trunc("  cancel", cols)),
        ];
        self.line_targets = vec![
            Target::None,
            Target::None,
            Target::MenuRemove(i),
            Target::MenuCancel,
        ];
        emit(out, usize::MAX);
    }

    fn render_confirm_switch(&mut self, i: usize) {
        let th = self.theme;
        let cols = self.cols;
        let label = self.label_of(i);
        let out = vec![
            styled(th.muted, false, &trunc("switch to", cols)),
            styled(th.accent, true, &trunc(&label, cols)),
            String::new(),
            styled(th.accent, true, &trunc("  switch", cols)),
            styled(th.dim, false, &trunc("  cancel", cols)),
        ];
        self.line_targets = vec![
            Target::None,
            Target::None,
            Target::None,
            Target::SwitchYes(i),
            Target::SwitchNo,
        ];
        emit(out, usize::MAX);
    }

    fn render_confirm_finish(&mut self, i: usize) {
        let th = self.theme;
        let cols = self.cols;
        let label = self.label_of(i);
        let out = vec![
            styled(th.error, true, &trunc(&format!("finish {label}?"), cols)),
            styled(th.dim, false, &trunc("stops session, trashes", cols)),
            styled(th.dim, false, &trunc("the workspace.", cols)),
            String::new(),
            styled(th.error, true, &trunc("  yes, finish", cols)),
            styled(th.dim, false, &trunc("  cancel", cols)),
        ];
        self.line_targets = vec![
            Target::None,
            Target::None,
            Target::None,
            Target::None,
            Target::FinishYes(i),
            Target::FinishNo,
        ];
        emit(out, usize::MAX);
    }

    fn label_of(&self, i: usize) -> String {
        self.workspaces
            .get(i)
            .map(|w| w.label.clone())
            .unwrap_or_default()
    }
}

fn emit(lines: Vec<String>, max_rows: usize) {
    let n = lines.len().min(max_rows);
    print!("{}", lines[..n].join("\r\n"));
}

fn styled(fg: Col, bold: bool, s: &str) -> String {
    let b = if bold { "\x1b[1m" } else { "" };
    format!("{b}{}{s}\x1b[0m", fg.fg())
}

/// A row of colored segments with a right-aligned trailer after `pad` spaces,
/// optionally filled with a background. Bold (if any) applies to the whole row;
/// a single reset closes it.
fn row(bg: Option<Col>, bold: bool, segs: &[(Col, &str)], pad: usize, trailer: (Col, &str)) -> String {
    let mut l = String::new();
    if let Some(b) = bg {
        l.push_str(&b.bg());
    }
    if bold {
        l.push_str("\x1b[1m");
    }
    for (c, s) in segs {
        l.push_str(&c.fg());
        l.push_str(s);
    }
    for _ in 0..pad {
        l.push(' ');
    }
    l.push_str(&trailer.0.fg());
    l.push_str(trailer.1);
    l.push_str("\x1b[0m");
    l
}

fn trunc(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else if max == 0 {
        String::new()
    } else {
        let mut t: String = s.chars().take(max.saturating_sub(1)).collect();
        t.push('…');
        t
    }
}
