<div align="center">
  <img src="softshell_turtle.png" width="96" alt="" />
  <h1>Cagalintry Launcher</h1>
  <p><em>A Minecraft launcher for our server, with modpacks that sync between players.</em></p>
</div>

---

> **Not an official Minecraft product.** Not approved by or associated with
> Mojang or Microsoft.

## What this is

A desktop Minecraft launcher built for a small private group. It does the usual
launcher things — instances, mod loaders, Modrinth content, Microsoft accounts —
and one thing most launchers don't: it keeps a modpack in sync across everyone
who plays on the server, without that modpack ever being published anywhere.

One person creates a pack. Private packs are visible only to their owner; public
packs appear for everyone with an account. When the owner adds, updates or
removes a mod, everyone else's **Play** button becomes **Update**. Updating is
always a deliberate click — the launcher never rewrites your instance behind
your back, and worlds are never touched.

Mods, resource packs and shader packs all sync. Config files sync too, with your
personal settings (options, keybinds, server list) protected from being
overwritten.

## Status

Early. The foundations are in place and the app runs; the launcher does not yet
launch Minecraft.

| Phase | | |
|---|---|---|
| 0 | Workspace, Tauri app, design system | ✅ done |
| 1 | Vanilla instance install + launch | 🚧 next |
| 2 | Microsoft accounts | |
| 3 | Full UI | |
| 4 | Fabric, Quilt, NeoForge | |
| 5 | Modrinth browsing and install | |
| 6 | Sync server | |
| 7 | Pack sync and the Update button | |
| 8 | Config overrides sync | |
| 9 | Packaging and auto-update | |

## Built with

- **[Rust](https://www.rust-lang.org/) 1.97** — everything below the UI
- **[Tauri](https://tauri.app/) 2.11** — desktop shell, ~10 MB instead of Electron's ~150 MB
- **[React](https://react.dev/) 19** + **TypeScript** + **[Tailwind CSS](https://tailwindcss.com/) 4** on **[Vite](https://vite.dev/) 8**

## Layout

```
crates/
  cagalintry-proto/     pack manifest + API types, shared with the sync server
  cagalintry-net/       hash-verified downloads, content-addressed cache
  cagalintry-mc/        version metadata, Java, launch arguments
  cagalintry-auth/      Microsoft → Xbox Live → Minecraft authentication
  cagalintry-modrinth/  Modrinth API v2 client
  cagalintry-sync/      pack diffing and applying updates
apps/
  launcher/             the Tauri application (React frontend + thin Rust layer)
```

The sync server is a separate self-hosted service and is **not** part of this
repository. It shares `cagalintry-proto` by path, so the types on the wire are
literally the same code on both ends.

## Building

Needs [Rust](https://rustup.rs/), [Node](https://nodejs.org/) 20+, and the
[Tauri prerequisites](https://tauri.app/start/prerequisites/) for your platform
(on Windows: the MSVC C++ build tools and WebView2, which ships with Windows 11).

```bash
npm install
npm run dev          # run the launcher
npm run build        # produce installers
cargo test --workspace
```

## Accounts and game ownership

Signing in with a Microsoft account that owns Minecraft is the only way to play.
The launcher implements the standard chain — Microsoft device code, Xbox Live,
XSTS, Minecraft services — and checks entitlements against
`api.minecraftservices.com/entitlements/mcstore` before a session is usable.

There is no offline mode, no placeholder credentials, and no build configuration
that produces a session any other way. A launch without a verified session is
refused rather than started.

The launcher never redistributes Minecraft. Every jar, library and asset is
downloaded from Mojang's own servers at install time, verified against the
hashes Mojang publishes.

## A note on other launchers

Cagalintry is not derived from any existing launcher. [Prism](https://prismlauncher.org/),
[Modrinth App](https://modrinth.com/app) and [Basalt](https://github.com/MegalithOfficial/basalt-launcher)
are all GPL-3 and their code has not been copied, adapted or transcribed here.
Where they've influenced this project it's at the level of *what a launcher
should do*, which is not something anyone owns.

## Licence

Copyright © Ricco0227. All rights reserved.

This source is published so the people who use the launcher can read it. No
licence to use, copy, modify or redistribute it is granted.
