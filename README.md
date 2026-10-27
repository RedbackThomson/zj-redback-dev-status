# zj-redback-dev-status

A Zellij sidebar plugin that lists `dev` workspaces by name and switches between
their Zellij sessions. It pairs with the `dev` CLI (jj workspaces + Zellij
sessions), whose session names are truncated and hashed; this plugin shows the
readable `repo/workspace` labels instead.

## What it does

- Renders workspaces grouped by repository, youngest-first, from
  `dev list --json`.
- Highlights the current session (accent + subtle bar); running workspaces get a
  green dot, stopped/absent ones a dim dot.
- Left-click a workspace → confirm → switch to its session (`switch_session`
  for a running one, `dev resume` for a dormant one).
- `[+ new]` → floating `dev start`.
- Right-click a workspace → confirm → `dev finish` (stops the session, trashes
  the workspace).
- Mouse-only and non-selectable, so it behaves like a status bar: it never keeps
  a session alive after its panes close.

## Colors

Colors follow the active Zellij theme (via `ModeUpdate`); muted tones are the
theme text blended toward its background. Roles can be pinned from the layout's
`plugin { }` config without recompiling:

```kdl
plugin location="file:...wasm" {
    color_accent  "#8caaee"
    color_running "green"
    color_dim     "8"
}
```

Keys: `color_text`, `color_muted`, `color_dim`, `color_accent`, `color_running`,
`color_error`, `color_hl_bg`. Values accept `#rrggbb`, a 0-255 8-bit index, or an
ANSI name (optionally `bright`). `demo_current "<repo/workspace>"` forces one row
to render as current — for previewing outside a real dev session.

## Build

```sh
nix build            # -> result/zj-redback-dev-status.wasm
nix develop -c cargo build --release --target wasm32-wasip1
```

## Preview

```sh
zellij -l layouts/local.kdl
```
