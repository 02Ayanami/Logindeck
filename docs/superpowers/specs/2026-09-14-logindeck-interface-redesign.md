# LoginDeck Interface and Brand Redesign

## Status

Approved in conversation on 2026-09-14 for implementation planning.

## Purpose

Replace AutoLogin's crowded prototype interface with a coherent desktop layout and rename the product to **LoginDeck**. The redesign must improve hierarchy, scanning, editing, and responsive behavior without changing the existing credential, discovery, persistence, or application-verification security boundaries.

## Scope

### Included

- Rename user-facing product text from AutoLogin to LoginDeck.
- Introduce a reusable desktop shell, sidebar navigation, page headers, lists/cards, detail panels, dialogs, notices, empty states, and loading skeletons.
- Redesign Websites, Applications, and Settings pages.
- Create a code-native LoginDeck mark and generate the platform icon set required by Tauri.
- Preserve English and Simplified Chinese localization parity; English remains the default.
- Add focused interaction, accessibility, responsive, secrecy, and build tests.

### Excluded

- Application launching or automatic-login execution.
- Browser-extension implementation.
- Changes to credential storage, Keychain access, SQLite schema, application discovery, signature verification, or process attestation.
- AI-provider/API-key settings that are not yet backed by functioning services.
- Changing the Tauri bundle identifier or data-directory identity.
- A dark theme. The first redesign ships one polished light theme, while tokens remain suitable for a later dark theme.

## Brand

### Name

The user-facing name is **LoginDeck**. The name describes an organized deck of accounts that can later support quick account switching.

The current bundle identifier remains `com.autologin.desktop` in this redesign so existing application identity and local data paths do not move unexpectedly. A separate migration is required before changing it.

### Mark

The mark combines two overlapping rounded account cards with a centered keyhole cutout. It must:

- remain recognizable at 16–32 px;
- use a simple silhouette with no text;
- avoid shields, fingerprints, browser logos, and literal padlock outlines;
- work on both light and dark surfaces;
- use deep navy, cobalt blue, a restrained cyan highlight, and white negative space;
- be implemented as a deterministic vector master rather than using the generated concept bitmap directly.

The vector master is the source for the macOS icon assets expected by the Tauri project.

### Voice

LoginDeck is calm, direct, and security-conscious. Labels prefer familiar verbs such as Add, Copy, Edit, Scan, Hide, and Delete. Security explanations remain short and factual; the interface avoids alarmist language.

## Visual System

### Layout tokens

- Sidebar width: 232 px expanded.
- Main content maximum width: 1280 px with fluid gutters.
- Spacing scale: 4, 8, 12, 16, 24, 32, and 48 px.
- Card radius: 12 px; dialog and detail-panel radius: 16 px.
- Use subtle one-pixel borders and restrained shadows; hierarchy comes primarily from spacing and typography.
- Primary accent: cobalt blue. Destructive actions use red only at the point of confirmation.

### Typography

Use the platform UI font stack. Page titles, section titles, body text, metadata, and labels have distinct sizes and weights. Paths and URLs may use a platform monospace stack where it improves scanning.

### Buttons and icons

- One visually primary action per page header.
- Secondary actions use neutral or ghost styling.
- Icon-only buttons require localized accessible names and tooltips.
- Common actions may appear as compact icons, but destructive actions never rely on color alone.
- Disabled and busy states remain visually distinct and preserve existing synchronous mutation guards.

## Application Shell

The desktop window uses a fixed left sidebar and a flexible main region.

### Sidebar

- LoginDeck mark and wordmark at the top.
- Websites, Applications, and Settings as the only primary destinations.
- The active destination uses a blue-tinted background, left accent, and `aria-current="page"`.
- The sidebar footer contains a short local-security statement, not operational status that could become stale.

### Main region

- A page header contains title, one-line description, search when relevant, and the primary action.
- Content uses a list/card master region.
- Selecting a website or application opens a right-hand detail panel instead of inserting editors inside list rows.

### Responsive behavior

- At narrow desktop widths, the sidebar collapses to icons while preserving tooltips and accessible labels.
- Below the compact breakpoint, the detail panel becomes a full-width view layered above the list with a clear Back action.
- Actions must not overflow with either English or Chinese labels.
- The project remains a desktop application; no separate mobile navigation is required.

## Websites Page

### Header

Contains page title, description, search, and Add Website. Search filters locally by name, URL, and username as today.

### Master list

Each row/card shows:

- a deterministic site monogram or neutral globe icon;
- website name;
- normalized domain/URL;
- username;
- Copy Username and Copy Password quick actions.

The password is never rendered. Clicking the non-action portion of the row opens the detail panel. Keyboard selection must offer the same behavior.

### Detail panel

The panel shows name, username, URL, and notes. It contains Open Website, Copy Username, Copy Password, Edit, and Delete. Edit switches the panel to a form; it does not insert a form into the list.

Add Website opens the same panel in creation mode. The password field remains local React state and is cleared immediately when credential IPC settles, before metadata refresh.

Delete opens a focused confirmation dialog. Existing committed-cleanup recovery semantics remain unchanged.

## Applications Page

### Header and tools

Contains title, description, search, Scan Applications, Add Application, and a Show Hidden filter. Scanning and import retain the current parent-wide single-flight behavior.

### Master list

Each row/card shows:

- application icon when safely available, otherwise a deterministic fallback;
- display name;
- automatic or manual source;
- available/unavailable state;
- account count;
- a compact Hide/Unhide action.

Long bundle identifiers and paths are removed from the primary row.

### Detail panel

The detail panel shows availability, bundle identifier, primary location, alternate locations, discovery source, and accounts. Paths wrap safely and may use monospace text.

Selecting an alternate location continues to route through verified import; the redesign must not expose an arbitrary path setter.

Account add/edit uses a focused form inside the panel. Account cards show label, username, and automatic-submission preference, never passwords. Existing mutation ownership, refresh-only recovery, and immediate password clearing remain intact.

Unavailable applications prominently offer Relink. Dangerous or uncommon actions appear at the bottom of the panel.

## Settings Page

Settings use clear section cards rather than a single loose control.

Initial sections:

1. **Language** — the existing persisted English/Simplified Chinese control.
2. **Security** — explanatory, read-only text describing system credential protection and user-presence behavior already implemented.
3. **About** — LoginDeck name and application version where it can be read safely.

Cloud AI provider, API key, browser capture, and automation controls are not displayed until corresponding services exist. The redesign must not create inert settings.

## Overlays and Feedback

### Detail panel

The detail panel is non-modal on wide screens and modal-like on compact screens. Focus moves to its heading when opened and returns to the originating row or Add button when closed.

### Dialogs

Deletion confirmations use a reusable accessible dialog with focus containment, Escape handling when not busy, explicit Cancel, and a destructive Confirm action.

### Notices

- Success notices may dismiss automatically after a short interval but remain announced through a polite live region.
- Errors persist until dismissed and use an assertive alert role where appropriate.
- Backend error codes remain translated through the existing safe mapping; raw errors and secret references never render.

### Loading and empty states

Initial lists use stable skeleton rows to avoid layout jumps. Mutations show local button/panel progress without replacing already loaded content. Empty states explain the next useful action.

## Component Boundaries

The redesign introduces focused presentation components without moving business logic into generic UI primitives:

- `AppShell` owns responsive navigation and page placement.
- `BrandMark` renders the deterministic vector mark.
- `PageHeader` provides page hierarchy and action slots.
- `VaultList` and `VaultRow` provide list semantics and visual consistency.
- `DetailPanel` owns focus entry/return and responsive presentation.
- `ConfirmDialog` owns accessible confirmation behavior.
- `Notice`, `EmptyState`, and `SkeletonList` provide state feedback.

Website and application feature components continue to own their Tauri commands, mutation sequencing, committed-cleanup recovery, and domain-specific forms. Generic components accept content and callbacks rather than importing Tauri APIs.

## Security and Data Invariants

- No password is displayed in list, detail, notice, log, or error content.
- Password inputs remain local, controlled state and are cleared at credential IPC settlement.
- The redesign adds no credential reveal command and no launch/automatic-login command.
- Existing exact Tauri command allowlist and URL opener scope remain unchanged.
- Application locations remain backend-validated; alternate selection still uses import validation.
- The bundle identifier and data-directory identity remain unchanged.
- Application helper/background filtering remains unchanged.

## Accessibility

- Landmarks, headings, list semantics, labels, live regions, and `aria-current` remain explicit.
- All functionality is keyboard reachable.
- Focus is restored after closing a panel or dialog.
- Icon-only controls have localized accessible names.
- Text and controls meet WCAG AA contrast targets.
- Motion is subtle and disabled or reduced under `prefers-reduced-motion`.

## Testing and Verification

### Component and interaction tests

- Sidebar navigation and current-page indication.
- Website and application row selection opens the correct detail panel.
- Add, edit, close, and focus-return flows.
- Modal deletion keyboard behavior.
- Parent mutation guards remain single-flight.
- Committed-cleanup and refresh-only recovery continue to work.
- Passwords never appear in rendered list/detail output and clear at IPC settlement.
- Search behavior remains correct.
- English and Chinese key parity and long-label rendering.

### Responsive and structural tests

- Wide split view, compact sidebar, and compact full-width detail modes.
- Primary actions remain visible without overflow at supported breakpoints.
- Static contract checks retain the exact command surface and safe capability policy.

### Build verification

- Frontend unit tests, TypeScript compilation, and Vite production build.
- Rust formatting, workspace tests, and Clippy with warnings denied.
- Tauri macOS debug app-only bundle.
- Render and inspect the major pages at wide and compact viewport sizes before completion.

Real application launch, Keychain, clipboard, and user-presence flows are excluded from automated visual acceptance.

## Rollout

The redesign lands as one coherent interface update because old inline editors and the new detail-panel interaction should not coexist. Existing stored websites, applications, accounts, settings, and credential references remain compatible because no persistence format changes.

Before public release, perform a dedicated trademark and store-name clearance for LoginDeck; the preliminary search performed during design is not legal clearance.
