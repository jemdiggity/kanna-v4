// kmux reference core: the model's state and its control-protocol handler
// (docs/kmux-spec.md §7). No DOM: index.html renders it, and
// tests/kmux-protocol runs the shared protocol cases against it in Node.
//
// The page (or a test) supplies these hooks:
const hooks = {
  render() {},
  /** Whether a pane is wider than tall, for `split: auto`. */
  paneIsWide(id) { return true; },
  desktopSize() { return { W: 1000, H: 600 }; },
};

// ---------------------------------------------------------------------------
// State
// window := { id, tabs, active, focused, zoomed, x, y, w, h, z }
// tab    := { id, root, title, lastFocus }   (title: "Tab 1", "Tab 2"… per window until renamed)
// node   := { t: 'pane', id } | { t: 'split', dir: 'row'|'column', kids: [{ node, size }] }
// Sizes are fractions of the parent split and always sum to 1.
// Empty containers close: a tab without panes, then a window without tabs.
// ---------------------------------------------------------------------------
const DEVICES = ['iPhone 16', 'iPhone 16 Pro', 'iPhone SE (3rd generation)', 'iPad Air (M2)'];
let S;

function freshState() {
  return { panes: {}, windows: [], key: null, urlEdit: null, servers: {}, n: { pane: 0, tab: 0, win: 0, req: 0, z: 0 } };
}

class KErr extends Error { constructor(code, message) { super(message); this.code = code; } }
const kerr = (code, msg) => new KErr(code, msg);

// ---------------------------------------------------------------------------
// Fractions
// ---------------------------------------------------------------------------
function frac(v) {
  if (typeof v === 'number') return v;
  const s = String(v ?? '').trim();
  let m;
  if ((m = /^(\d*\.?\d+)\/(\d*\.?\d+)$/.exec(s))) return Number(m[1]) / Number(m[2]);
  if ((m = /^(\d*\.?\d+)%$/.exec(s))) return Number(m[1]) / 100;
  if (/^\d*\.?\d+$/.test(s)) return Number(s);
  throw kerr('bad_request', `"${s}" is not a fraction (try 1/3)`);
}
function fmtFrac(x) {
  if (Math.abs(x - 1) < 1e-6) return '1';
  for (let d = 2; d <= 8; d++) {
    const n = Math.round(x * d);
    if (n > 0 && Math.abs(x - n / d) < 0.004) return `${n}/${d}`;
  }
  return `${Math.round(x * 100)}%`;
}

// ---------------------------------------------------------------------------
// Windows and tabs
// ---------------------------------------------------------------------------
const winById = id => S.windows.find(w => w.id === id);
const keyWin = () => winById(S.key);
const allTabs = () => S.windows.flatMap(w => w.tabs);
const tabById = id => allTabs().find(t => t.id === id);
const winOfTab = t => S.windows.find(w => w.tabs.includes(t));
const activeTabOf = w => w && (w.tabs.find(t => t.id === w.active) || w.tabs[0]);

function makeWindow() {
  const { W, H } = hooks.desktopSize();
  const k = keyWin();
  let geo = k ? { x: k.x + 32, y: k.y + 32, w: k.w, h: k.h } : { x: 24, y: 18, w: W - 48, h: H - 36 };
  if (geo.x + geo.w > W || geo.y + geo.h > H) geo = { x: 24, y: 18, w: Math.min(geo.w, W - 48), h: Math.min(geo.h, H - 36) };
  const w = { id: 'w' + (++S.n.win), tabs: [], tabCount: 0, active: null, focused: null, zoomed: null, ...geo, z: ++S.n.z };
  S.windows.push(w);
  S.key = w.id;
  return w;
}
function makeTab(w, activate = true) {
  const t = { id: 't' + (++S.n.tab), root: null, title: `Tab ${++w.tabCount}`, lastFocus: null };
  w.tabs.push(t);
  if (activate || !w.active) w.active = t.id;
  return t;
}
function raise(w) { w.z = ++S.n.z; S.key = w.id; }
function targetWin(ref) {
  if (ref == null) return keyWin() || makeWindow();
  if (ref === 'new') return makeWindow();
  const w = winById(ref);
  if (!w) throw kerr('not_found', `no window "${ref}"`);
  return w;
}
function dropEmpty() {
  for (const w of S.windows) {
    const at = Math.max(0, w.tabs.findIndex(t => t.id === w.active));
    w.tabs = w.tabs.filter(t => t.root);
    if (!w.tabs.some(t => t.id === w.active)) w.active = w.tabs[Math.min(at, w.tabs.length - 1)]?.id ?? null;
    const ids = paneIds(activeTabOf(w)?.root);
    if (!ids.includes(w.focused)) w.focused = ids.includes(activeTabOf(w)?.lastFocus) ? activeTabOf(w).lastFocus : ids[0] ?? null;
    if (w.zoomed && !ids.includes(w.zoomed)) w.zoomed = null;
  }
  S.windows = S.windows.filter(w => w.tabs.length);
  if (!keyWin()) S.key = [...S.windows].sort((a, b) => b.z - a.z)[0]?.id ?? null;
}

// ---------------------------------------------------------------------------
// Layout trees
// ---------------------------------------------------------------------------
function locate(node, id, parent = null, index = -1) {
  if (!node) return null;
  if (node.t === 'pane') return node.id === id ? { node, parent, index } : null;
  for (let i = 0; i < node.kids.length; i++) {
    const r = locate(node.kids[i].node, id, node, i);
    if (r) return r;
  }
  return null;
}
function findPane(id) {
  for (const win of S.windows) for (const tab of win.tabs) {
    const loc = locate(tab.root, id);
    if (loc) return { win, tab, ...loc };
  }
  return null;
}
const paneIds = node => !node ? [] : node.t === 'pane' ? [node.id] : node.kids.flatMap(k => paneIds(k.node));
function normalize(node) {
  if (!node || node.t === 'pane') return node;
  node.kids.forEach(k => { k.node = normalize(k.node); });
  return node.kids.length === 1 ? node.kids[0].node : node;
}
function detach(id) {
  const f = findPane(id);
  if (!f) return;
  if (!f.parent) f.tab.root = null;
  else {
    const gone = f.parent.kids[f.index].size;
    f.parent.kids.splice(f.index, 1);
    const n = f.parent.kids.length;
    f.parent.kids.forEach(k => { k.size = gone < 1 ? k.size / (1 - gone) : 1 / n; });
    f.tab.root = normalize(f.tab.root);
  }
  if (f.win.zoomed === id) f.win.zoomed = null;
}
function toTree(node, size) {
  if (!node) return null;
  const out = node.t === 'pane'
    ? { pane: S.panes[node.id].name || node.id }
    : { split: node.dir, children: node.kids.map(k => toTree(k.node, k.size)) };
  if (size != null && size < 1 - 1e-6) out.size = fmtFrac(size);
  return out;
}
const tabTitle = t => t ? t.title : '';
const resolvePane = ref => Object.values(S.panes).find(p => p.id === ref || p.name === ref);
function need(ref) {
  if (!ref) throw kerr('bad_request', 'missing pane');
  const p = resolvePane(ref);
  if (!p) throw kerr('not_found', `no pane "${ref}"`);
  return p;
}
function needTab(ref) {
  const t = tabById(ref);
  if (!t) throw kerr('not_found', `no tab "${ref}"`);
  return t;
}
const label = p => p.name ? `${p.name} (${p.id})` : p.id;

function place(paneId, w, { split = 'auto', size = 0.5, tab = false }) {
  const node = { t: 'pane', id: paneId };
  const t = tab || !w.tabs.length ? makeTab(w) : activeTabOf(w);
  w.zoomed = null;
  if (!t.root) { t.root = node; return; }
  const ids = paneIds(t.root);
  const target = ids.includes(w.focused) ? w.focused : ids[ids.length - 1];
  const dir = split === 'right' ? 'row' : split === 'down' ? 'column' : autoDir(target);
  insertBeside(t, target, node, dir, true, size);
}
function insertBeside(t, targetId, node, dir, after, size) {
  const loc = locate(t.root, targetId);
  if (loc.parent && loc.parent.dir === dir) {
    const share = loc.parent.kids[loc.index].size;
    loc.parent.kids[loc.index].size = share * (1 - size);
    loc.parent.kids.splice(loc.index + (after ? 1 : 0), 0, { node, size: share * size });
  } else {
    const kids = [{ node: loc.node, size: 1 - size }, { node, size }];
    if (!after) kids.reverse();
    const sp = { t: 'split', dir, kids };
    if (loc.parent) loc.parent.kids[loc.index].node = sp; else t.root = sp;
  }
}
function appendToTab(t, id) {
  const node = { t: 'pane', id };
  if (!t.root) t.root = node;
  else if (t.root.t === 'split' && t.root.dir === 'row') {
    const n = t.root.kids.length;
    t.root.kids.forEach(k => { k.size *= n / (n + 1); });
    t.root.kids.push({ node, size: 1 / (n + 1) });
  } else t.root = { t: 'split', dir: 'row', kids: [{ node: t.root, size: 1 / 2 }, { node, size: 1 / 2 }] };
}
function autoDir(id) { return hooks.paneIsWide(id) ? 'row' : 'column'; }

// ---------------------------------------------------------------------------
// Panes and lifecycle: starting -> running | failed; running -> exited; * -> closed
// ---------------------------------------------------------------------------
function createPane(type, a) {
  const p = {
    id: 'p' + (++S.n.pane), name: a.name || null, type, state: 'starting', error: null, gen: 0, waiters: [],
    cmd: a.cmd || null, cwd: a.cwd || '~', url: a.url ? normUrl(a.url) : null,
    app: a.app || null, device: a.device || 'iPhone 16', lines: [], draft: '', busy: false, server: null, exitCode: null,
  };
  S.panes[p.id] = p;
  return p;
}
function setState(p, state, extra = {}) {
  Object.assign(p, extra, { state });
  if (state !== 'starting') p.waiters.splice(0).forEach(fn => fn());
  hooks.render();
}
const settled = p => p.state !== 'starting' ? Promise.resolve() : new Promise(r => p.waiters.push(r));
function later(p, ms, fn) { const g = p.gen; setTimeout(() => { if (p.gen === g && S.panes[p.id] === p) fn(); }, ms); }

function start(p) {
  p.gen++;
  stopServer(p);
  p.error = null; p.exitCode = null;
  setState(p, 'starting');
  if (p.type === 'term') {
    p.lines = [];
    later(p, 150, () => { setState(p, 'running'); if (p.cmd) runTerm(p, p.cmd); });
  } else if (p.type === 'web') {
    later(p, 400, () => setState(p, 'running'));
  } else if (p.type === 'ios') {
    if (!DEVICES.includes(p.device)) {
      later(p, 600, () => setState(p, 'failed', { error: `Unknown device "${p.device}".\nAvailable: ${DEVICES.join(', ')}` }));
    } else {
      later(p, 1800, () => setState(p, 'running'));
    }
  }
}
function kill(p) {
  p.gen++;
  stopServer(p);
  p.waiters.splice(0).forEach(fn => fn());
  delete S.panes[p.id];
  p.state = 'closed';
}

// --- simulated terminal ----------------------------------------------------
const prompt = p => `${p.cwd} % `;
function termPrint(p, text, cls = '') { p.lines.push({ text, cls }); if (p.lines.length > 400) p.lines.shift(); }
function runTerm(p, line) {
  termPrint(p, prompt(p) + line, 'cmd');
  const [c, ...rest] = line.trim().split(/\s+/);
  const arg = rest.join(' ');
  if (!c) return hooks.render();
  if (c === 'clear') p.lines = [];
  else if (c === 'echo') termPrint(p, arg);
  else if (c === 'pwd') termPrint(p, p.cwd);
  else if (c === 'cd') p.cwd = arg || '~';
  else if (c === 'ls') termPrint(p, 'README.md  build  node_modules  package.json  src');
  else if (c === 'help') termPrint(p, 'simulated shell: echo, ls, pwd, cd, clear, npm run dev (ctrl+c stops), exit [code]');
  else if (c === 'exit') { setState(p, 'exited', { exitCode: Number(rest[0]) || 0 }); return; }
  else if (line.trim() === 'npm run dev' || line.trim() === 'npm start') startServer(p);
  else termPrint(p, `zsh: command not found: ${c}`, 'err');
  hooks.render();
}
function startServer(p) {
  p.busy = true;
  const host = 'localhost:3000';
  const steps = [[0, '> myapp@1.0.0 dev'], [0, '> vite --port 3000'], [500, ''], [400, '  VITE v6.0.0  ready in 312 ms', 'ok'], [0, ''], [0, '  ➜  Local:   http://localhost:3000/']];
  let t = 0;
  steps.forEach(([d, txt, cls], i) => {
    t += d;
    later(p, t, () => {
      if (!p.busy) return;
      termPrint(p, txt, cls);
      if (i === steps.length - 1) { S.servers[host] = p.id; p.server = host; }
      hooks.render();
    });
  });
}
function stopServer(p) {
  if (p.server) { delete S.servers[p.server]; p.server = null; }
  p.busy = false;
}
function interrupt(p) {
  if (!p.busy) { termPrint(p, prompt(p) + '^C'); hooks.render(); return; }
  termPrint(p, '^C');
  stopServer(p);
  hooks.render();
}

// --- web helpers -----------------------------------------------------------
function normUrl(u) { return /^[a-z]+:\/\//i.test(u) ? u : 'http://' + u; }
function hostOf(u) { try { return new URL(u).host; } catch { return u; } }
const isLocal = u => /^(localhost|127\.0\.0\.1)(:\d+)?$/.test(hostOf(u));

function summary(p) {
  const s = { id: p.id, name: p.name, type: p.type, state: p.state };
  if (p.type === 'term') Object.assign(s, { cmd: p.cmd, cwd: p.cwd });
  if (p.type === 'web') s.url = p.url;
  if (p.type === 'ios') Object.assign(s, { app: p.app, device: p.device });
  if (p.exitCode != null) s.exitCode = p.exitCode;
  if (p.error) s.error = p.error;
  return s;
}

// ---------------------------------------------------------------------------
// kmux core: handles control-protocol requests (see docs/kmux-spec.md §7)
// ---------------------------------------------------------------------------
function focusPane(p) {
  const f = findPane(p.id);
  f.win.active = f.tab.id;
  if (f.win.zoomed && f.win.zoomed !== p.id) f.win.zoomed = null;
  f.win.focused = p.id;
  f.tab.lastFocus = p.id;
  raise(f.win);
}

const COMMANDS = ['capabilities', 'open', 'arrange', 'move', 'move-tab', 'rename-tab', 'resize', 'list', 'focus', 'zoom', 'close', 'restart', 'send', 'navigate'];

async function handle(req) {
  const { id, cmd, args = {} } = req;
  const ok = x => ({ id, ok: true, ...x });
  try {
    switch (cmd) {
      case 'capabilities':
        return ok({ mux: 'kmux', paneTypes: ['term', 'web', 'ios'], commands: COMMANDS,
          features: ['windows', 'tabs', 'fractionalSizing', 'namedPanes', 'zoom', 'move', 'lifecycle'] });

      case 'open': {
        const { type } = args;
        if (!['term', 'web', 'ios'].includes(type)) throw kerr('bad_request', `unknown pane type "${type}" (term, web or ios)`);
        if (args.name && resolvePane(args.name)) throw kerr('name_taken', `a pane named "${args.name}" already exists`);
        if (args.split && !['right', 'down', 'auto'].includes(args.split)) throw kerr('bad_request', 'split must be right, down or auto');
        const size = args.size == null ? 0.5 : frac(args.size);
        if (!(size > 0 && size < 1)) throw kerr('bad_request', 'size must be a fraction between 0 and 1');
        if (type === 'web' && !args.url) throw kerr('bad_request', 'web panes need a url');
        if (type === 'ios' && !args.app) throw kerr('bad_request', 'ios panes need an app');
        const w = targetWin(args.window);
        const p = createPane(type, args);
        place(p.id, w, { split: args.split || 'auto', size, tab: !!args.tab });
        w.focused = p.id;
        findPane(p.id).tab.lastFocus = p.id;
        raise(w);
        start(p);
        const res = () => ({ pane: summary(p), window: findPane(p.id)?.win.id ?? w.id });
        if (args.wait === false) return ok(res());
        await settled(p);
        if (p.state === 'failed') throw kerr('start_failed', p.error);
        return ok(res());
      }

      case 'arrange': {
        const used = [];
        const build = n => {
          if (n && n.pane != null) {
            const p = need(n.pane);
            if (used.includes(p.id)) throw kerr('layout_invalid', `"${n.pane}" appears more than once`);
            used.push(p.id);
            return { t: 'pane', id: p.id };
          }
          if (!n || !['row', 'column'].includes(n.split) || !Array.isArray(n.children) || !n.children.length)
            throw kerr('layout_invalid', 'malformed layout tree');
          const sizes = n.children.map(c => c.size == null ? null : frac(c.size));
          if (sizes.some(s => s != null && !(s > 0 && s <= 1))) throw kerr('layout_invalid', 'sizes must be fractions between 0 and 1');
          const given = sizes.reduce((a, s) => a + (s ?? 0), 0);
          const unsized = sizes.filter(s => s == null).length;
          if (given > 1 + 1e-9) throw kerr('layout_invalid', `sizes in a ${n.split} add up to ${fmtFrac(given)}, more than 1`);
          if (unsized && given >= 1 - 1e-9) throw kerr('layout_invalid', `no space left for the unsized panes in a ${n.split}`);
          const fill = unsized ? (1 - given) / unsized : 0;
          const scale = unsized ? 1 : 1 / given;
          return { t: 'split', dir: n.split, kids: n.children.map((c, i) => ({ node: build(c), size: (sizes[i] ?? fill) * scale })) };
        };
        const root = normalize(build(args.layout));
        const w = targetWin(args.window);
        const t = activeTabOf(w) || makeTab(w);
        used.forEach(detach);
        const leftovers = paneIds(t.root);
        t.root = root;
        if (leftovers.length) {
          const nt = makeTab(w, false);
          nt.title = 'unarranged';
          nt.root = normalize({ t: 'split', dir: 'row', kids: leftovers.map(id => ({ node: { t: 'pane', id }, size: 1 / leftovers.length })) });
        }
        w.zoomed = null;
        w.active = t.id;
        if (!used.includes(w.focused)) w.focused = used[0];
        raise(w);
        dropEmpty();
        hooks.render();
        return ok({ window: w.id, layout: toTree(t.root) });
      }

      case 'move': {
        const p = need(args.pane);
        const from = findPane(p.id);
        if (args.to) {
          const target = need(args.to);
          const side = args.side || 'swap';
          if (!['left', 'right', 'top', 'bottom', 'swap'].includes(side)) throw kerr('bad_request', 'side must be left, right, top, bottom or swap');
          if (target.id === p.id) return ok({ window: from.win.id });
          if (side === 'swap') {
            const b = findPane(target.id);
            from.node.id = target.id;
            b.node.id = p.id;
            if (from.win.focused === p.id) from.win.focused = target.id;
          } else {
            detach(p.id);
            insertBeside(findPane(target.id).tab, target.id, { t: 'pane', id: p.id },
              side === 'left' || side === 'right' ? 'row' : 'column', side === 'right' || side === 'bottom', 0.5);
          }
        } else {
          let dest;
          if (args.tab && args.tab !== 'new') dest = needTab(args.tab);
          else if (args.tab === 'new') dest = makeTab(args.window ? targetWin(args.window) : from.win);
          else if (args.window) { const w = targetWin(args.window); dest = w.tabs.length ? activeTabOf(w) : makeTab(w); }
          else throw kerr('bad_request', 'move needs to + side, tab, or window');
          if (dest === from.tab) return ok({ window: from.win.id });
          detach(p.id);
          appendToTab(dest, p.id);
        }
        focusPane(p);
        dropEmpty();
        hooks.render();
        const f = findPane(p.id);
        return ok({ window: f.win.id, tab: f.tab.id, layout: toTree(f.tab.root) });
      }

      case 'move-tab': {
        const t = needTab(args.tab);
        const src = winOfTab(t);
        const dest = targetWin(args.window ?? src.id);
        let index = args.index == null ? dest.tabs.length : Number(args.index);
        if (!Number.isInteger(index) || index < 0) throw kerr('bad_request', 'index must be a whole number ≥ 0');
        const fromIdx = src.tabs.indexOf(t);
        src.tabs.splice(fromIdx, 1);
        if (src === dest && index > fromIdx) index--;
        dest.tabs.splice(Math.min(index, dest.tabs.length), 0, t);
        dest.active = t.id;
        const ids = paneIds(t.root);
        dest.focused = ids.includes(t.lastFocus) ? t.lastFocus : ids[0];
        dest.zoomed = null;
        raise(dest);
        dropEmpty();
        hooks.render();
        return ok({ window: dest.id, tabs: dest.tabs.map(x => x.id) });
      }

      case 'rename-tab': {
        const t = needTab(args.tab);
        const title = String(args.title ?? '').trim();
        if (!title) throw kerr('bad_request', 'a tab title cannot be empty');
        t.title = title;
        hooks.render();
        return ok({ tab: t.id, title });
      }

      case 'resize': {
        const p = need(args.pane);
        const s = frac(args.size);
        if (!(s > 0 && s < 1)) throw kerr('bad_request', 'size must be a fraction between 0 and 1');
        const f = findPane(p.id);
        if (!f.parent) throw kerr('bad_request', `${label(p)} fills its tab, so there is nothing to resize against`);
        const old = f.parent.kids[f.index].size;
        f.parent.kids.forEach((k, i) => { if (i !== f.index) k.size = k.size * (1 - s) / (1 - old); });
        f.parent.kids[f.index].size = s;
        hooks.render();
        return ok({ layout: toTree(f.tab.root) });
      }

      case 'list':
        return ok({
          windows: S.windows.map(w => ({
            id: w.id, key: w.id === S.key,
            focused: w.focused ? S.panes[w.focused].name || w.focused : null,
            zoomed: w.zoomed ? S.panes[w.zoomed].name || w.zoomed : null,
            tabs: w.tabs.map(t => ({ id: t.id, title: tabTitle(t), active: t.id === w.active, layout: toTree(t.root) })),
          })),
          panes: Object.values(S.panes).map(summary),
        });

      case 'focus': {
        if (args.pane) focusPane(need(args.pane));
        else if (args.tab) {
          const t = needTab(args.tab), w = winOfTab(t), ids = paneIds(t.root);
          w.active = t.id;
          w.zoomed = null;
          w.focused = ids.includes(t.lastFocus) ? t.lastFocus : ids[0];
          raise(w);
        } else if (args.window) raise(targetWin(args.window));
        else throw kerr('bad_request', 'focus needs a pane, tab or window');
        hooks.render();
        return ok({ window: S.key });
      }

      case 'zoom': {
        const p = need(args.pane);
        const was = findPane(p.id).win.zoomed === p.id;
        focusPane(p);
        const w = findPane(p.id).win;
        w.zoomed = was ? null : p.id;
        hooks.render();
        return ok({ zoomed: !!w.zoomed });
      }

      case 'close': {
        let victims;
        if (args.pane) victims = [need(args.pane)];
        else if (args.tab) victims = paneIds(needTab(args.tab).root).map(i => S.panes[i]);
        else if (args.window) { const w = targetWin(args.window); victims = w.tabs.flatMap(t => paneIds(t.root)).map(i => S.panes[i]); }
        else throw kerr('bad_request', 'close needs a pane, tab or window');
        victims.forEach(p => { detach(p.id); kill(p); });
        dropEmpty();
        hooks.render();
        return ok({ closed: victims.map(p => p.id) });
      }

      case 'restart': {
        const p = need(args.pane);
        start(p);
        await settled(p);
        if (p.state === 'failed') throw kerr('start_failed', p.error);
        return ok({ pane: summary(p) });
      }

      case 'send': {
        const p = need(args.pane);
        if (p.type !== 'term') throw kerr('wrong_type', `${label(p)} is a ${p.type} pane; send only works on term panes`);
        if (p.state !== 'running') throw kerr('bad_request', `${label(p)} is ${p.state}`);
        if (p.busy) { termPrint(p, args.text ?? ''); hooks.render(); } else runTerm(p, args.text ?? '');
        return ok({});
      }

      case 'navigate': {
        const p = need(args.pane);
        if (p.type !== 'web') throw kerr('wrong_type', `${label(p)} is a ${p.type} pane; navigate only works on web panes`);
        if (!args.url) throw kerr('bad_request', 'missing url');
        p.url = normUrl(args.url);
        p.urlDraft = null;
        start(p);
        await settled(p);
        return ok({ pane: summary(p) });
      }

      default:
        throw kerr('bad_request', `unknown command "${cmd}"`);
    }
  } catch (e) {
    if (e instanceof KErr) { dropEmpty(); hooks.render(); return { id, ok: false, error: { code: e.code, message: e.message } }; }
    throw e;
  }
}
