(() => {
  const parentOrigin = document.currentScript.dataset.parent;
  const post = msg => parent.postMessage(msg, parentOrigin);
  const rect = el => { const r = el.getBoundingClientRect(); return { left: r.left, top: r.top, width: r.width, height: r.height }; };
  const info = () => ({ hash: location.hash.slice(0, 100), scroll: [Math.round(scrollX), Math.round(scrollY)], viewport: [innerWidth, innerHeight] });
  const textOf = el => (el.innerText || el.getAttribute("aria-label") || el.getAttribute("alt") || "").replace(/\s+/g, " ").trim().slice(0, 120);

  function selectorOf(el) {
    const parts = [];
    for (let n = el; n && n.nodeType === 1 && n.localName !== "html"; n = n.parentElement) {
      if (n.id && /^[A-Za-z][\w-]*$/.test(n.id)) { parts.unshift("#" + n.id); break; }
      let part = n.localName;
      const same = n.parentElement ? [...n.parentElement.children].filter(c => c.localName === n.localName) : [];
      if (same.length > 1) part += `:nth-of-type(${same.indexOf(n) + 1})`;
      parts.unshift(part);
    }
    return parts.join(" > ");
  }
  function hit(x, y) {
    const el = document.elementFromPoint(x, y);
    if (!el || el === document.documentElement || el === document.body) return null;
    return { selector: selectorOf(el), text: textOf(el), rect: rect(el) };
  }
  function locate(item) {
    if (item.selector) {
      let el = null;
      try { el = document.querySelector(item.selector); } catch {}
      const text = el ? (el.innerText || "").replace(/\s+/g, " ").trim() : "";
      if (el && (!item.text || text.startsWith(item.text.replace(/…$/, "")))) {
        const r = rect(el);
        if (r.width || r.height) return r;
      }
    }
    const [x, y, w, h] = item.box;
    return { left: x - scrollX, top: y - scrollY, width: w, height: h };
  }
  async function snapshot(box) {
    if (!window.html2canvas) return null;
    try {
      const c = await html2canvas(document.documentElement, { x: scrollX, y: scrollY, width: innerWidth, height: innerHeight, windowWidth: innerWidth, windowHeight: innerHeight, scrollX: -scrollX, scrollY: -scrollY, scale: Math.min(2, devicePixelRatio || 1), logging: false, useCORS: true });
      const g = c.getContext("2d"), k = c.width / innerWidth;
      const line = Math.max(3, Math.round(c.width / 400));
      g.lineWidth = line;
      g.strokeStyle = "#ff3b30";
      g.strokeRect(box.left * k + line / 2, box.top * k + line / 2, box.width * k - line, box.height * k - line);
      return c.toDataURL("image/png");
    } catch { return null; }
  }

  addEventListener("message", async e => {
    if (e.source !== parent || e.origin !== parentOrigin || !e.data || typeof e.data !== "object") return;
    const m = e.data, reply = body => post({ seq: m.seq, ...body });
    if (m.kind === "hit") reply({ hit: hit(m.x, m.y) });
    if (m.kind === "locate") reply({ info: info(), boxes: Object.fromEntries((m.items || []).map(i => [i.id, locate(i)])) });
    if (m.kind === "snapshot") reply({ data: await snapshot(m.box) });
    if (m.kind === "hash") { if (location.hash !== m.hash) location.hash = m.hash; reply({ info: info() }); }
    if (m.kind === "scroll") { scrollTo(m.x, m.y); reply({ info: info() }); }
    if (m.kind === "info") reply({ info: info() });
  });
  const changed = () => post({ kind: "changed", info: info() });
  addEventListener("scroll", changed, { passive: true });
  addEventListener("resize", changed);
  addEventListener("hashchange", changed);
  addEventListener("load", changed);
  for (const type of ["keydown", "keyup"]) addEventListener(type, e => post({ kind: "key", meta: e.metaKey, key: e.key }), true);
  post({ kind: "ready", info: info() });
})();
