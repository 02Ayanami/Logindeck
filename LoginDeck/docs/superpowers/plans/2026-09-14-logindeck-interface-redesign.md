# LoginDeck Interface and Brand Redesign Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the AutoLogin prototype UI with the approved LoginDeck desktop shell, master/detail workflows, bilingual polished presentation, and deterministic platform icon set without changing credential or persistence boundaries.

**Architecture:** Keep website and application command ownership inside their existing feature components, but move reusable presentation and focus behavior into small `components/` units. `AppShell` owns navigation and responsive placement; feature pages own selection, mutation sequencing, forms, and Tauri calls. The redesign is presentation-only: existing Tauri commands, Zod wire validation, Rust storage, Keychain behavior, and bundle identity remain unchanged.

**Tech Stack:** React 19, TypeScript 5.9, React Testing Library, Vitest, i18next, CSS media/container queries, Tauri 2, Rust 1.98.

**Spec:** `docs/superpowers/specs/2026-09-14-logindeck-interface-redesign.md`

## Global Constraints

- The visible product name is `LoginDeck`; the bundle identifier remains exactly `com.autologin.desktop`.
- English remains the default locale and every new English key must have a Simplified Chinese peer.
- Do not add launch, automatic-login, password-reveal, browser-capture, AI-provider, or API-key behavior.
- Do not change the Tauri command allowlist, URL opener scope, SQLite schema, Keychain access, application verification, signature checks, or process attestation.
- Password plaintext stays in local controlled React state and is cleared as soon as credential IPC settles, before metadata refresh.
- Alternate application locations continue through `importApplications`; never add a direct path setter.
- Use the approved light theme: 232 px expanded sidebar, 1280 px content maximum, spacing scale 4/8/12/16/24/32/48 px, 12 px cards, and 16 px panels/dialogs.
- Generic components accept content and callbacks and must not import `app/src/lib/tauri.ts`.
- Keep parent-wide single-flight mutation guards and committed-cleanup/refresh-only recovery behavior intact.
- All icon-only controls require localized accessible names and tooltips; all interactions remain keyboard reachable.

## File Structure

- `app/src/components/brand/BrandMark.tsx` — deterministic inline vector mark used by the shell and About card.
- `app/src/components/shell/AppShell.tsx` — sidebar navigation, responsive shell, and page placement.
- `app/src/components/ui/PageHeader.tsx` — title, description, search slot, and action slot.
- `app/src/components/ui/VaultList.tsx` — accessible list and selectable row presentation.
- `app/src/components/ui/DetailPanel.tsx` — wide split panel, compact overlay, focus entry, focus return.
- `app/src/components/ui/ConfirmDialog.tsx` — focus-contained destructive confirmation.
- `app/src/components/ui/Feedback.tsx` — notices, empty state, and stable loading skeletons.
- `app/src/components/ui/ui.test.tsx` — accessibility and focus contracts for shared primitives.
- `app/src/app/App.tsx` — page state and composition only.
- `app/src/app/app.css` — tokens, shell, shared components, feature layout, breakpoints, reduced motion.
- `app/src/features/websites/WebsiteList.tsx` — website master/detail state and existing mutations.
- `app/src/features/websites/WebsiteEditor.tsx` — reusable create/edit panel form.
- `app/src/features/applications/ApplicationLibrary.tsx` — application master/detail state, scan/import ownership.
- `app/src/features/applications/ApplicationAccounts.tsx` — account list and focused account form inside detail panel.
- `app/src/features/applications/ApplicationImport.tsx` — import buttons and verified import path flow.
- `app/src/features/settings/SettingsPage.tsx` — Language, Security, and About cards.
- `app/src/locales/en.json`, `app/src/locales/zh-CN.json` — complete bilingual UI copy.
- `app/src-tauri/icons/logindeck.svg` — vector icon master.
- `app/src-tauri/icons/**` — Tauri-generated platform icon assets.
- `app/src-tauri/tauri.conf.json` — visible product/window title only; identifier unchanged.

---

### Task 1: LoginDeck Brand Mark and Desktop Shell

**Files:**
- Create: `app/src/components/brand/BrandMark.tsx`
- Create: `app/src/components/shell/AppShell.tsx`
- Create: `app/src/components/shell/AppShell.test.tsx`
- Modify: `app/src/app/App.tsx`
- Modify: `app/src/app/App.test.tsx`
- Modify: `app/src/app/app.css`
- Modify: `app/src/locales/en.json`
- Modify: `app/src/locales/zh-CN.json`

**Interfaces:**
- Consumes: `Page = 'websites' | 'applications' | 'settings'` from `App.tsx` initially; move and export it from `AppShell.tsx`.
- Produces: `BrandMark({ className?, title? })`, `AppShell({ page, onNavigate, children })`, and exported `Page`.

- [ ] **Step 1: Write failing shell tests**

```tsx
// app/src/components/shell/AppShell.test.tsx
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import { AppShell } from './AppShell';

describe('AppShell', () => {
  it('shows the LoginDeck brand and marks the active destination', () => {
    render(<AppShell page="websites" onNavigate={vi.fn()}><p>content</p></AppShell>);
    expect(screen.getByText('LoginDeck')).toBeVisible();
    expect(screen.getByRole('link', { name: 'Websites' })).toHaveAttribute('aria-current', 'page');
    expect(screen.getByText('Protected by your system credential store.')).toBeVisible();
  });

  it('navigates without exposing navigation as feature logic', async () => {
    const onNavigate = vi.fn();
    render(<AppShell page="websites" onNavigate={onNavigate}><p>content</p></AppShell>);
    await userEvent.click(screen.getByRole('link', { name: 'Applications' }));
    expect(onNavigate).toHaveBeenCalledWith('applications');
  });
});
```

Update `App.test.tsx` to expect `LoginDeck` and the three navigation destinations rather than `AutoLogin`.

- [ ] **Step 2: Run the focused tests and verify RED**

Run:

```bash
cd app && pnpm test --run src/components/shell/AppShell.test.tsx src/app/App.test.tsx
```

Expected: FAIL because `AppShell.tsx` does not exist and the current heading is `AutoLogin`.

- [ ] **Step 3: Implement the vector mark and shell**

Implement `BrandMark` as an SVG with two overlapping rounded card paths and a centered white keyhole path. Use `viewBox="0 0 64 64"`, `aria-hidden={!title}`, and a `<title>` only when `title` is supplied. Do not use a bitmap or external asset.

Implement `AppShell` with:

```ts
export type Page = 'websites' | 'applications' | 'settings';
export type AppShellProps = {
  page: Page;
  onNavigate: (page: Page) => void;
  children: React.ReactNode;
};
```

Use an `<aside>`, `<nav aria-label={t('app.navigation')}>`, anchor-like buttons for all three pages, `aria-current="page"` on the active destination, and `<main id="main-content">`. Add a localized footer security statement. Update `App.tsx` so it owns only `page`, locale initialization, and feature selection inside `AppShell`.

Add root CSS custom properties for all approved colors, spacing, radii, type, borders, and shadows; add `.app-shell`, `.sidebar`, `.sidebar__brand`, `.sidebar__nav`, `.sidebar__footer`, and `.main-region` rules.

- [ ] **Step 4: Add bilingual brand and shell keys**

Set both `app.title` values to `LoginDeck`. Add peer keys under `app`:

```json
// English values
"securityFooter": "Protected by your system credential store.",
"skipToContent": "Skip to content"
```

```json
// Simplified Chinese values
"securityFooter": "由系统凭据存储提供保护。",
"skipToContent": "跳到主要内容"
```

- [ ] **Step 5: Run shell, i18n, and full frontend tests**

Run:

```bash
cd app && pnpm test --run src/components/shell/AppShell.test.tsx src/app/App.test.tsx src/app/i18n.test.ts
cd app && pnpm test --run
```

Expected: all tests PASS and English/Chinese flattened key sets remain equal.

- [ ] **Step 6: Commit**

```bash
git add app/src/components/brand app/src/components/shell app/src/app/App.tsx app/src/app/App.test.tsx app/src/app/app.css app/src/locales/en.json app/src/locales/zh-CN.json
git commit -m "feat: add LoginDeck desktop shell"
```

---

### Task 2: Shared Page, List, and Feedback Primitives

**Files:**
- Create: `app/src/components/ui/PageHeader.tsx`
- Create: `app/src/components/ui/VaultList.tsx`
- Create: `app/src/components/ui/Feedback.tsx`
- Create: `app/src/components/ui/ui.test.tsx`
- Modify: `app/src/app/app.css`

**Interfaces:**
- Consumes: localized strings and feature-owned callbacks passed as props.
- Produces: `PageHeader`, `VaultList`, `VaultRow`, `Notice`, `EmptyState`, and `SkeletonList`.

- [ ] **Step 1: Write failing primitive tests**

```tsx
import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { EmptyState, Notice, SkeletonList } from './Feedback';
import { PageHeader } from './PageHeader';
import { VaultList, VaultRow } from './VaultList';

describe('shared presentation primitives', () => {
  it('renders one page heading and named action regions', () => {
    render(<PageHeader title="Websites" description="Saved sign-ins" search={<input aria-label="Search websites" />} actions={<button>Add website</button>} />);
    expect(screen.getByRole('heading', { level: 1, name: 'Websites' })).toBeVisible();
    expect(screen.getByRole('button', { name: 'Add website' })).toBeVisible();
  });

  it('keeps list and row semantics explicit', () => {
    render(<VaultList label="Saved websites"><VaultRow selected={false}><span>Example</span></VaultRow></VaultList>);
    expect(screen.getByRole('list', { name: 'Saved websites' })).toBeVisible();
    expect(screen.getByRole('listitem')).toHaveTextContent('Example');
  });

  it('uses correct live-region roles and stable skeleton count', () => {
    const { rerender } = render(<Notice kind="success">Saved</Notice>);
    expect(screen.getByRole('status')).toHaveTextContent('Saved');
    rerender(<Notice kind="error">Failed</Notice>);
    expect(screen.getByRole('alert')).toHaveTextContent('Failed');
    rerender(<SkeletonList label="Loading websites" rows={3} />);
    expect(screen.getAllByTestId('skeleton-row')).toHaveLength(3);
  });

  it('provides an actionable empty state', () => {
    render(<EmptyState title="No websites" description="Add your first website." action={<button>Add website</button>} />);
    expect(screen.getByRole('button', { name: 'Add website' })).toBeVisible();
  });
});
```

- [ ] **Step 2: Run test and verify RED**

Run: `cd app && pnpm test --run src/components/ui/ui.test.tsx`

Expected: FAIL because the shared modules do not exist.

- [ ] **Step 3: Implement minimal typed primitives**

`PageHeader` accepts `title: string`, `description: string`, and optional `search`/`actions: ReactNode`. `VaultList` renders a named `<ul>`. `VaultRow` renders `<li>` and applies `data-selected={selected}` without owning click behavior. `Notice` renders `role="alert"` for errors and `role="status"` for success/info. `SkeletonList` renders a single status label and exactly `rows` decorative skeleton rows. `EmptyState` accepts explicit title, description, and action slots.

Add CSS for `.page-header`, `.vault-list`, `.vault-row`, `.notice`, `.empty-state`, and `.skeleton-row`. Use one-pixel borders and spacing for hierarchy; do not add feature-specific selectors to these components.

- [ ] **Step 4: Run focused and full tests**

```bash
cd app && pnpm test --run src/components/ui/ui.test.tsx
cd app && pnpm test --run
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add app/src/components/ui app/src/app/app.css
git commit -m "feat: add LoginDeck presentation primitives"
```

---

### Task 3: Accessible Detail Panel and Confirmation Dialog

**Files:**
- Create: `app/src/components/ui/DetailPanel.tsx`
- Create: `app/src/components/ui/ConfirmDialog.tsx`
- Modify: `app/src/components/ui/ui.test.tsx`
- Modify: `app/src/app/app.css`

**Interfaces:**
- Produces: `DetailPanel({ title, open, onClose, returnFocusRef, children, footer? })`.
- Produces: `ConfirmDialog({ title, description, confirmLabel, cancelLabel, busy, onConfirm, onCancel })`.

- [ ] **Step 1: Add failing focus tests**

```tsx
it('moves focus into a detail panel and returns it to the opener', async () => {
  const opener = createRef<HTMLButtonElement>();
  const { rerender } = render(<><button ref={opener}>Open</button><DetailPanel open={false} title="Example" onClose={() => {}} returnFocusRef={opener}>Details</DetailPanel></>);
  opener.current?.focus();
  rerender(<><button ref={opener}>Open</button><DetailPanel open title="Example" onClose={() => {}} returnFocusRef={opener}>Details</DetailPanel></>);
  expect(screen.getByRole('heading', { name: 'Example' })).toHaveFocus();
  rerender(<><button ref={opener}>Open</button><DetailPanel open={false} title="Example" onClose={() => {}} returnFocusRef={opener}>Details</DetailPanel></>);
  expect(opener.current).toHaveFocus();
});

it('contains dialog focus and ignores Escape while busy', async () => {
  const onCancel = vi.fn();
  const { rerender } = render(<ConfirmDialog title="Delete website?" description="Cannot be undone." confirmLabel="Delete" cancelLabel="Cancel" busy={false} onConfirm={() => {}} onCancel={onCancel} />);
  expect(screen.getByRole('button', { name: 'Delete' })).toHaveFocus();
  await userEvent.keyboard('{Escape}');
  expect(onCancel).toHaveBeenCalledOnce();
  onCancel.mockClear();
  rerender(<ConfirmDialog title="Delete website?" description="Cannot be undone." confirmLabel="Delete" cancelLabel="Cancel" busy onConfirm={() => {}} onCancel={onCancel} />);
  await userEvent.keyboard('{Escape}');
  expect(onCancel).not.toHaveBeenCalled();
});
```

Import `createRef`, `userEvent`, and `vi` in `ui.test.tsx`.

- [ ] **Step 2: Run test and verify RED**

Run: `cd app && pnpm test --run src/components/ui/ui.test.tsx`

Expected: FAIL because `DetailPanel` and `ConfirmDialog` do not exist.

- [ ] **Step 3: Implement focus-safe overlays**

Use `useLayoutEffect` in `DetailPanel`: focus the `tabIndex={-1}` `<h2>` when `open` becomes true; restore `returnFocusRef.current` when it becomes false or unmounts. Render a close button with a caller-provided localized label.

Render `ConfirmDialog` through `createPortal(..., document.body)`, with `role="dialog"`, `aria-modal="true"`, `aria-labelledby`, and `aria-describedby`. Initially focus the destructive confirm button. Trap Tab/Shift+Tab between cancel and confirm. Escape calls `onCancel` only when `busy === false`; both buttons are disabled while busy.

Add `.detail-panel`, `.detail-panel__backdrop`, `.confirm-dialog`, and compact overlay styles. At compact width the panel fills the main region; on wide screens its backdrop is transparent and it occupies the right column.

- [ ] **Step 4: Verify component and full suites**

```bash
cd app && pnpm test --run src/components/ui/ui.test.tsx
cd app && pnpm test --run
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add app/src/components/ui/DetailPanel.tsx app/src/components/ui/ConfirmDialog.tsx app/src/components/ui/ui.test.tsx app/src/app/app.css
git commit -m "feat: add accessible LoginDeck overlays"
```

---

### Task 4: Website Master/Detail Workflow

**Files:**
- Modify: `app/src/features/websites/WebsiteList.tsx`
- Modify: `app/src/features/websites/WebsiteEditor.tsx`
- Modify: `app/src/features/websites/WebsiteList.test.tsx`
- Modify: `app/src/locales/en.json`
- Modify: `app/src/locales/zh-CN.json`
- Modify: `app/src/app/app.css`

**Interfaces:**
- Consumes: `PageHeader`, `VaultList`, `VaultRow`, `DetailPanel`, `ConfirmDialog`, and feedback primitives.
- Preserves: all existing `tauri.listWebsites`, `saveWebsite`, `deleteWebsite`, copy, and `openUrl` calls and payloads.
- Produces: selected website ID plus panel mode `'view' | 'create' | 'edit'` owned by `WebsiteList`.

- [ ] **Step 1: Add failing master/detail tests before changing markup**

Add tests to `WebsiteList.test.tsx`:

```tsx
it('opens website details from the row without rendering a password', async () => {
  listResponse(website);
  render(<WebsiteList />);
  await userEvent.click(await screen.findByRole('button', { name: 'View Example' }));
  expect(screen.getByRole('heading', { name: 'Example' })).toBeVisible();
  expect(screen.getByText('https://example.com/login')).toBeVisible();
  expect(document.body.textContent).not.toContain('fixture-password');
});

it('uses the same panel for create and edit and restores focus', async () => {
  listResponse(website);
  render(<WebsiteList />);
  const add = await screen.findByRole('button', { name: 'Add website' });
  await userEvent.click(add);
  expect(screen.getByRole('heading', { name: 'Add website' })).toBeVisible();
  await userEvent.click(screen.getByRole('button', { name: 'Close details' }));
  expect(add).toHaveFocus();
  await userEvent.click(screen.getByRole('button', { name: 'View Example' }));
  await userEvent.click(screen.getByRole('button', { name: 'Edit website' }));
  expect(screen.getByRole('heading', { name: 'Edit website' })).toBeVisible();
});
```

Retain every existing mutation, validation, malformed response, cleanup recovery, single-flight, search, clipboard, and password-settlement test; update selectors only where the interaction intentionally moves into the panel/dialog.

- [ ] **Step 2: Run website tests and verify RED**

Run: `cd app && pnpm test --run src/features/websites/WebsiteList.test.tsx`

Expected: new tests FAIL because website rows still use inline editors and lack `View Example`/`Close details` controls.

- [ ] **Step 3: Refactor presentation without changing mutation sequencing**

In `WebsiteList`, store:

```ts
type WebsitePanelMode = 'view' | 'create' | 'edit';
const [selectedId, setSelectedId] = useState<string>();
const [panelMode, setPanelMode] = useState<WebsitePanelMode>('view');
const openerRef = useRef<HTMLElement | null>(null);
```

Render a `PageHeader` with search and one primary Add Website button. Render each website as a `VaultRow` containing monogram, name, normalized URL, username, quick Copy Username, and Copy Password actions. The non-action row control must be a real button named `View {{name}}`; quick actions call `stopPropagation` and never open the panel.

Use `DetailPanel` for view/create/edit. In view mode show URL, username, notes, Open Website, copy actions, Edit, and Delete. Move `WebsiteEditor` into create/edit panel mode. Replace inline delete confirmation with `ConfirmDialog`.

Do not change the existing `try/finally` points that clear password state at IPC settlement. Do not add password text to view-mode props or DOM.

- [ ] **Step 4: Add bilingual website detail keys**

Add matching keys for `description`, `savedList`, `view`, `closeDetails`, `editAction`, `details`, `domain`, `noNotes`, and `back`. Use concise English and natural Simplified Chinese; preserve all old keys still referenced by tests or code.

- [ ] **Step 5: Run website and full frontend suites**

```bash
cd app && pnpm test --run src/features/websites/WebsiteList.test.tsx src/app/i18n.test.ts
cd app && pnpm test --run
```

Expected: PASS, including existing cleanup and plaintext-clearing tests.

- [ ] **Step 6: Commit**

```bash
git add app/src/features/websites app/src/locales/en.json app/src/locales/zh-CN.json app/src/app/app.css
git commit -m "feat: redesign website vault workflow"
```

---

### Task 5: Application Master/Detail Workflow

**Files:**
- Modify: `app/src/features/applications/ApplicationLibrary.tsx`
- Modify: `app/src/features/applications/ApplicationAccounts.tsx`
- Modify: `app/src/features/applications/ApplicationImport.tsx`
- Modify: `app/src/features/applications/ApplicationLibrary.test.tsx`
- Modify: `app/src/locales/en.json`
- Modify: `app/src/locales/zh-CN.json`
- Modify: `app/src/app/app.css`

**Interfaces:**
- Consumes: all shared shell/list/panel/dialog/feedback primitives.
- Preserves: parent-wide busy guard, `rescanApplications`, verified `importApplications`, hide/unhide, account mutation, cleanup recovery, and password settlement semantics.
- Produces: selected application ID and detail/account panel modes owned by `ApplicationLibrary`.

- [ ] **Step 1: Add failing application detail tests**

Add focused tests using the existing application fixture:

```tsx
it('keeps long identifiers out of the primary row and shows them in details', async () => {
  listResponse(application);
  render(<ApplicationLibrary />);
  const row = await screen.findByRole('listitem');
  expect(within(row).queryByText(application.platform_application_id)).toBeNull();
  await userEvent.click(within(row).getByRole('button', { name: `View ${application.display_name}` }));
  expect(screen.getByText(application.platform_application_id)).toBeVisible();
  expect(screen.getByText(application.launch_target)).toBeVisible();
});

it('shows unavailable applications with a relink action', async () => {
  listResponse({ ...application, is_present: false });
  render(<ApplicationLibrary />);
  await userEvent.click(await screen.findByRole('button', { name: `View ${application.display_name}` }));
  expect(screen.getByRole('button', { name: 'Relink application' })).toBeVisible();
});

it('never renders an account password in list or details', async () => {
  listResponse({ ...application, accounts: [{ ...account, password: 'must-not-render' }] });
  render(<ApplicationLibrary />);
  await screen.findByText(application.display_name);
  expect(document.body.textContent).not.toContain('must-not-render');
});
```

Use the fixture property names already defined in the test file; do not add a password to the production `ApplicationAccount` type.

- [ ] **Step 2: Run application tests and verify RED**

Run: `cd app && pnpm test --run src/features/applications/ApplicationLibrary.test.tsx`

Expected: new detail and relink assertions FAIL against the current inline layout.

- [ ] **Step 3: Build the application master/detail page**

Render a `PageHeader` with search, Show Hidden, Scan Applications, and an Add Application menu containing Import `.app` and Import Folder. Keep Scan as the single primary action and keep all scan/import controls disabled under the same parent busy flag.

Rows show safe icon fallback, display name, discovery source, present/unavailable state, account count, and Hide/Unhide. The row control opens `DetailPanel`; bundle identifier and paths only render there.

The detail panel shows availability, bundle identifier, primary path, alternate locations, discovery source, and accounts. Alternate-location selection must call the existing verified import callback. When unavailable, place `Relink application` prominently and route it through the existing picker/import flow.

Move account add/edit forms inside the panel through `ApplicationAccounts`. Account cards show label, username, and auto-submit preference only. Use `ConfirmDialog` for account deletion. Preserve password clearing at command settlement before refresh.

- [ ] **Step 4: Add bilingual application detail copy**

Add matching keys for `description`, `savedList`, `view`, `closeDetails`, `details`, `available`, `accountCount`, `primaryLocation`, `alternateLocations`, `bundleIdentifier`, `discoverySource`, `relink`, `addApplication`, `back`, and icon action labels.

- [ ] **Step 5: Run focused and full frontend tests**

```bash
cd app && pnpm test --run src/features/applications/ApplicationLibrary.test.tsx src/app/i18n.test.ts
cd app && pnpm test --run
```

Expected: PASS, including the pre-existing parent single-flight, verified import, refresh-only recovery, and password clearing tests.

- [ ] **Step 6: Commit**

```bash
git add app/src/features/applications app/src/locales/en.json app/src/locales/zh-CN.json app/src/app/app.css
git commit -m "feat: redesign application library workflow"
```

---

### Task 6: Settings Sections and Product Metadata

**Files:**
- Modify: `app/src/features/settings/SettingsPage.tsx`
- Modify: `app/src/features/settings/SettingsPage.test.tsx`
- Modify: `app/src/features/settings/LanguageSetting.tsx`
- Modify: `app/src/locales/en.json`
- Modify: `app/src/locales/zh-CN.json`
- Modify: `app/src-tauri/tauri.conf.json`
- Modify: `app/src/app/app.css`

**Interfaces:**
- Consumes: existing `LanguageSetting` persistence flow and `BrandMark`.
- Produces: three visible section cards only: Language, Security, About.

- [ ] **Step 1: Add failing settings tests**

```tsx
it('renders Language, Security, and About without inert future settings', async () => {
  render(<SettingsPage />);
  expect(screen.getByRole('heading', { name: 'Language' })).toBeVisible();
  expect(screen.getByRole('heading', { name: 'Security' })).toBeVisible();
  expect(screen.getByRole('heading', { name: 'About' })).toBeVisible();
  expect(screen.getByText('LoginDeck')).toBeVisible();
  expect(screen.queryByText(/API key/i)).toBeNull();
  expect(screen.queryByText(/AI provider/i)).toBeNull();
});
```

Retain the existing persisted locale and error behavior tests.

- [ ] **Step 2: Run tests and verify RED**

Run: `cd app && pnpm test --run src/features/settings/SettingsPage.test.tsx`

Expected: FAIL because Security and About cards do not exist.

- [ ] **Step 3: Implement the three settings cards**

Use a page header and `.settings-grid`. Keep `LanguageSetting` functional and unchanged in command semantics. Security is read-only factual text describing system credential storage and user-presence authorization. About shows `BrandMark`, `LoginDeck`, and version `0.1.0`. Do not show AI, API key, browser capture, or automation controls.

In `tauri.conf.json`, change only:

```json
"productName": "LoginDeck"
```

and the window title to `LoginDeck`. Assert manually and in diff that `identifier` remains `com.autologin.desktop`.

- [ ] **Step 4: Add bilingual settings copy and verify key parity**

Add peer keys for `description`, `security`, `securityDescription`, `securityCredentialStore`, `securityPresence`, `about`, `aboutDescription`, `version`, and `versionValue`.

Run:

```bash
cd app && pnpm test --run src/features/settings/SettingsPage.test.tsx src/app/i18n.test.ts
```

Expected: PASS.

- [ ] **Step 5: Verify Tauri identity did not change**

Run:

```bash
grep -n 'productName\|title\|identifier' app/src-tauri/tauri.conf.json
```

Expected output contains `LoginDeck` for product/title and exactly `com.autologin.desktop` for identifier.

- [ ] **Step 6: Commit**

```bash
git add app/src/features/settings app/src/locales/en.json app/src/locales/zh-CN.json app/src-tauri/tauri.conf.json app/src/app/app.css
git commit -m "feat: redesign LoginDeck settings"
```

---

### Task 7: Deterministic LoginDeck Platform Icons

**Files:**
- Create: `app/src-tauri/icons/logindeck.svg`
- Modify: `app/src-tauri/icons/32x32.png`
- Modify: `app/src-tauri/icons/64x64.png`
- Modify: `app/src-tauri/icons/128x128.png`
- Modify: `app/src-tauri/icons/128x128@2x.png`
- Modify: `app/src-tauri/icons/icon.png`
- Modify: `app/src-tauri/icons/icon.icns`
- Modify: `app/src-tauri/icons/icon.ico`
- Modify: all generated Square, Android, and iOS assets under `app/src-tauri/icons/`

**Interfaces:**
- Consumes: the same geometry/color values used by `BrandMark`.
- Produces: one deterministic vector master and the standard Tauri icon set.

- [ ] **Step 1: Add a failing icon contract check**

Run before creating the master:

```bash
test -f app/src-tauri/icons/logindeck.svg && grep -q 'viewBox="0 0 512 512"' app/src-tauri/icons/logindeck.svg
```

Expected: FAIL because `logindeck.svg` does not exist.

- [ ] **Step 2: Create the vector master**

Create a 512×512 SVG with no text, external references, filters, embedded raster data, shield, fingerprint, browser logo, or literal padlock outline. Use a deep navy rear rounded card, cobalt front rounded card, restrained cyan edge/highlight, and a centered white negative-space keyhole. Keep all shapes inside a safe 48 px inset and use integer coordinates so regeneration is deterministic.

- [ ] **Step 3: Verify the vector contract**

Run:

```bash
test -f app/src-tauri/icons/logindeck.svg
grep -q 'viewBox="0 0 512 512"' app/src-tauri/icons/logindeck.svg
! grep -Eqi '<text|<image|href=|filter=' app/src-tauri/icons/logindeck.svg
```

Expected: all commands exit 0.

- [ ] **Step 4: Generate the Tauri icon set**

Run from `app/`:

```bash
pnpm exec tauri icon src-tauri/icons/logindeck.svg
```

Expected: Tauri regenerates the macOS assets under `src-tauri/icons/`.

- [ ] **Step 5: Inspect critical sizes**

Open `32x32.png`, `128x128.png`, `icon.png`, and `icon.icns` in Finder/Preview. Confirm the two-card silhouette and keyhole remain recognizable at 32 px, no edge is clipped, and colors match the shell mark.

- [ ] **Step 6: Commit**

```bash
git add app/src-tauri/icons
git commit -m "feat: add LoginDeck platform icon set"
```

---

### Task 8: Responsive Layout, Long Labels, and Reduced Motion

**Files:**
- Modify: `app/src/app/app.css`
- Modify: `app/src/components/shell/AppShell.test.tsx`
- Modify: `app/src/components/ui/ui.test.tsx`
- Modify: `app/src/features/websites/WebsiteList.test.tsx`
- Modify: `app/src/features/applications/ApplicationLibrary.test.tsx`

**Interfaces:**
- Consumes: stable `data-*`/class hooks from shell, rows, and panels.
- Produces: wide split view, compact icon sidebar, and compact full-width detail behavior.

- [ ] **Step 1: Add failing structural and long-label tests**

Add assertions that the shell exposes `data-layout="desktop"`, the panel exposes `data-detail-panel`, row action containers use an accessible label, and Chinese translations render all header actions. Add a fixture with a 200-character application name and a long path; assert both remain in the DOM and action buttons remain reachable by role.

Add a CSS contract test in `ui.test.tsx` that reads `app.css` through `readFileSync` and requires:

```ts
expect(css).toContain('@media (max-width: 900px)');
expect(css).toContain('@media (max-width: 680px)');
expect(css).toContain('@media (prefers-reduced-motion: reduce)');
expect(css).toContain('overflow-wrap: anywhere');
```

- [ ] **Step 2: Run tests and verify RED**

Run:

```bash
cd app && pnpm test --run src/components/shell/AppShell.test.tsx src/components/ui/ui.test.tsx src/features/websites/WebsiteList.test.tsx src/features/applications/ApplicationLibrary.test.tsx
```

Expected: FAIL until the required responsive contracts and long-content styles exist.

- [ ] **Step 3: Implement responsive CSS**

At widths above 900 px, use 232 px sidebar and a list/detail grid capped at 1280 px. At 900 px and below, collapse the sidebar to 72 px, visually hide wordmark/labels while retaining accessible names/tooltips, and keep header actions wrapped. At 680 px and below, render detail panels as full-width overlays over the main region with an explicit Back/Close action.

Add `min-width: 0` to flexible grid children, `overflow-wrap: anywhere` to paths/URLs, and `flex-wrap: wrap` to header actions. Under `prefers-reduced-motion: reduce`, set transition/animation durations to `0.01ms` and iteration count to `1`.

- [ ] **Step 4: Run focused and full tests**

```bash
cd app && pnpm test --run src/components/shell/AppShell.test.tsx src/components/ui/ui.test.tsx src/features/websites/WebsiteList.test.tsx src/features/applications/ApplicationLibrary.test.tsx
cd app && pnpm test --run
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add app/src/app/app.css app/src/components/shell/AppShell.test.tsx app/src/components/ui/ui.test.tsx app/src/features/websites/WebsiteList.test.tsx app/src/features/applications/ApplicationLibrary.test.tsx
git commit -m "feat: polish LoginDeck responsive behavior"
```

---

### Task 9: Full Security, Build, and Visual Verification

**Files:**
- Modify only files required to correct failures revealed by this task.
- Do not weaken assertions, allowlists, schemas, lint levels, or secret-handling tests to obtain a pass.

**Interfaces:**
- Consumes: the completed redesign.
- Produces: verified frontend, Rust workspace, Tauri bundle, and visual acceptance evidence.

- [ ] **Step 1: Verify no user-facing AutoLogin text remains**

Run:

```bash
grep -RIn --exclude-dir=node_modules --exclude-dir=dist --exclude-dir=target --exclude='*.md' 'AutoLogin' app/src app/src-tauri
```

Expected: no user-facing source or Tauri config matches. Internal crate/module names and bundle identifier remain unchanged where intentionally preserved.

- [ ] **Step 2: Run the complete frontend verification**

```bash
cd app && pnpm test --run
cd app && pnpm build
```

Expected: all Vitest tests PASS; TypeScript and Vite production build exit 0.

- [ ] **Step 3: Run complete Rust verification**

From the repository root with the configured Rust environment:

```bash
cargo fmt --all --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

Expected: formatting clean, all tests PASS, and Clippy exits 0 with warnings denied.

- [ ] **Step 4: Verify Tauri security contracts**

```bash
cargo test --manifest-path app/src-tauri/Cargo.toml --test command_contract
git diff 37e1083 -- app/src-tauri/capabilities/default.json app/src-tauri/src/commands app/src/lib/tauri.ts migrations crates
```

Expected: command contract PASS. The diff contains no new launch/password-reveal commands, capability expansion, persistence migration, or credential-layer change.

- [ ] **Step 5: Build the macOS debug app bundle**

Run:

```bash
cd app && pnpm exec tauri build --debug --bundles app
```

Expected: exits 0 and creates the LoginDeck `.app` debug bundle.

- [ ] **Step 6: Render and inspect major pages**

Start Vite and Tauri in separate VS Code terminals:

```bash
cd app && pnpm dev
```

```bash
cd app && pnpm exec tauri dev
```

Inspect Websites, Applications, and Settings at approximately 1280×800, 900×700, and 680×700. Verify: no action overflow in English or Chinese; sidebar collapse; detail overlay/back flow; focus visibility; stable skeletons; long URL/path wrapping; correct LoginDeck mark; and no password text rendered.

- [ ] **Step 7: Review final diff and commit verification fixes**

```bash
git status --short
git diff --check
git diff --stat 37e1083..HEAD
```

If verification required code corrections, commit only those corrections:

```bash
git add -u
git commit -m "fix: complete LoginDeck verification"
```

Do not add `.DS_Store`, `.pnpm-store/`, or unrelated generated files.
