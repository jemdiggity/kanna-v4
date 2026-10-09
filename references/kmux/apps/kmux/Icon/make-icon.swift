#!/usr/bin/env swift
// Draws the kmux app icon with Core Graphics and Core Text, so the icon has no binary-only source.
//
// The icon (default, "kmux"): "km" over "ux" on Kanna.app's near-white tile, each letter in one
// of Kanna's icon gradients, the green cursor pill as the bar, letters in Fira Mono Bold
// (bundled: apps/kmux/Icon/fonts/FiraMono-Bold.ttf, SIL OFL 1.1, so no download is needed).
// At 16 px it becomes four gradient quadrants. Everything else here is the exploration that
// led to it, kept selectable with --variant / --font-file.
//
//   swift apps/kmux/Icon/make-icon.swift                     # the icon -> apps/kmux/Icon/kmux.icns
//   swift apps/kmux/Icon/make-icon.swift --variant light-row # another variant
//   swift apps/kmux/Icon/make-icon.swift --preview sheet.png # preview sheet of every variant
//   swift apps/kmux/Icon/make-icon.swift --out DIR           # where kmux.icns (and the .iconset) go
//   swift apps/kmux/Icon/make-icon.swift --sheet out.png [--only v01,v02]  # contact sheet: 512/64/32/16 px, light + dark
//   swift apps/kmux/Icon/make-icon.swift --zoom out.png [--only v01,v02]   # 32 and 16 px blown up to judge pixels
//   swift apps/kmux/Icon/make-icon.swift --sheet2 out.png [--only v11,v12] # round two: 32 px lettered and as quadrants
//   swift apps/kmux/Icon/make-icon.swift --family out.png --kanna kanna.png [--only v11,v13]  # beside Kanna.app's icon
//   --quad N: draw v11-v16 as four quadrants up to N px (default 16)
//
// Older variants: dark-stack (the first icon), dark-row, light-row ("km|ux" in one line; reads best large, smears below 32 px).
// v01-v10, v11-v16 (Kanna.app's icon palette): explorations for choosing a new icon (see "Explorations" below); selectable with --variant.
//
// Palette: Kanna's UI tokens (kanna-v3 apps/kanna3-mac/Sources/Kanna3Mac/Theme.swift).
// Letters k m u x are warn, bad, machine, accent: the amber -> red -> purple -> blue
// run of Kanna.app's own icon. The "|" is a neutral token (text), as a cursor / split
// divider, so it doesn't compete with the letters (a horizontal split in the stack layout).

import AppKit
import CoreText
import Foundation

// MARK: Palette

struct Palette {
    let bgTop: CGColor, bgBottom: CGColor, edge: CGColor
    let k: CGColor, m: CGColor, u: CGColor, x: CGColor, bar: CGColor
}

func rgb(_ v: UInt32, _ a: CGFloat = 1) -> CGColor {
    CGColor(srgbRed: CGFloat((v >> 16) & 0xff) / 255, green: CGFloat((v >> 8) & 0xff) / 255,
            blue: CGFloat(v & 0xff) / 255, alpha: a)
}

// Theme.swift: bg 14161a/ffffff, panel 1b1e24/f4f5f7, panel2 22262d/eaecf0, line 2c313a/d8dce2,
// text e6e8eb/1d2026, muted 8b93a0/6b7280, accent 7aa2ff/2f63d8, ok 5fcf93/1f9d5c,
// warn f0b35a/b7791f, bad ef6b6b/cf3f3f, machine b48cff/7c4dcc.
let dark = Palette(bgTop: rgb(0x22262d), bgBottom: rgb(0x14161a), edge: rgb(0xffffff, 0.10),
                   k: rgb(0xf0b35a), m: rgb(0xef6b6b), u: rgb(0xb48cff), x: rgb(0x7aa2ff), bar: rgb(0xe6e8eb))
let light = Palette(bgTop: rgb(0xffffff), bgBottom: rgb(0xeaecf0), edge: rgb(0x000000, 0.10),
                    k: rgb(0xb7791f), m: rgb(0xcf3f3f), u: rgb(0x7c4dcc), x: rgb(0x2f63d8), bar: rgb(0x1d2026))

// MARK: Explorations v01-v10
//
// Ten directions for the user to pick from (layout, the bar, font, background, colour use).
// Each keeps one Kanna colour per letter. Below 64 px each simplifies the way Apple's icons
// do: bigger, heavier letters, no hairlines or highlights, lines snapped to whole pixels.

/// Render state handed to a variant's draw function. The context is already scaled so all
/// geometry is in 1024 units; `px` is one device pixel in those units.
struct R {
    let ctx: CGContext, size: Int
    var scale: CGFloat { CGFloat(size) / 1024 }
    var px: CGFloat { 1024 / CGFloat(size) }
    var small: Bool { size <= 32 }   // Finder / list sizes
    var tiny: Bool { size <= 16 }
    /// Rounds a 1024-unit coordinate to a device pixel at small sizes.
    func pix(_ v: CGFloat) -> CGFloat { small ? (v / px).rounded() * px : v }
    func pix(_ r: CGRect) -> CGRect { small ? snap(r, scale) : r }
    /// A line width of at least `minPx` device pixels.
    func line(_ w: CGFloat, minPx: CGFloat = 1) -> CGFloat { max(w, minPx * px) }
}

let ok = (dark: rgb(0x5fcf93), light: rgb(0x1f9d5c))   // Theme.swift "ok" green
let tileRect = CGRect(x: 100, y: 100, width: 824, height: 824)

func sysFont(_ size: CGFloat, _ weight: NSFont.Weight, _ design: NSFontDescriptor.SystemDesign) -> CTFont {
    let base = NSFont.systemFont(ofSize: size, weight: weight)
    let d = base.fontDescriptor.withDesign(design) ?? base.fontDescriptor
    return (NSFont(descriptor: d, size: size) ?? base) as CTFont
}

func glyphPath(_ s: String, _ f: CTFont) -> CGPath {
    var u = Array(s.utf16)
    var g = [CGGlyph](repeating: 0, count: u.count)
    CTFontGetGlyphsForCharacters(f, &u, &g, u.count)
    return CTFontCreatePathForGlyph(f, g[0], nil) ?? CGMutablePath()   // empty if the font lacks the glyph
}

func moved(_ p: CGPath, _ dx: CGFloat, _ dy: CGFloat) -> CGPath {
    var t = CGAffineTransform(translationX: dx, y: dy)
    return p.copy(using: &t)!
}

struct Glyph { let path: CGPath, color: CGColor, letter: String }

/// Lays letters out in rows: each glyph's ink is centred on its column x, rows sit `lead`
/// apart on shared baselines, and the whole block's ink is centred on `centre`.
func grid(_ rows: [[(String, CGColor)]], _ f: CTFont, cols: [CGFloat], lead: CGFloat,
          centre: CGPoint, _ r: R) -> [Glyph] {
    var out: [Glyph] = []
    var box = CGRect.null
    for (i, row) in rows.enumerated() {
        for (j, (ch, c)) in row.enumerated() {
            let p = glyphPath(ch, f)
            let b = p.boundingBoxOfPath
            let q = moved(p, cols[j] - b.midX, -CGFloat(i) * lead)
            box = box.union(q.boundingBoxOfPath)
            out.append(Glyph(path: q, color: c, letter: ch))
        }
    }
    let dx = r.pix(centre.x - box.midX), dy = r.pix(centre.y - box.midY)
    return out.map { Glyph(path: moved($0.path, dx, dy), color: $0.color, letter: $0.letter) }
}

/// Proportional-font rows: glyphs set by their advances plus `tracking`, each row's ink
/// centred on x = 512, rows `lead` apart, the block's ink centred on `centreY`.
func words(_ rows: [[(String, CGColor)]], _ f: CTFont, tracking: CGFloat, lead: CGFloat, centreY: CGFloat, _ r: R) -> [Glyph] {
    var out: [Glyph] = []
    var box = CGRect.null
    for (i, row) in rows.enumerated() {
        var x: CGFloat = 0
        var line: [Glyph] = []
        var lb = CGRect.null
        for (ch, c) in row {
            var u = Array(ch.utf16), g: CGGlyph = 0, adv = CGSize.zero
            CTFontGetGlyphsForCharacters(f, &u, &g, 1)
            CTFontGetAdvancesForGlyphs(f, .horizontal, &g, &adv, 1)
            let q = moved(glyphPath(ch, f), x, -CGFloat(i) * lead)
            lb = lb.union(q.boundingBoxOfPath)
            line.append(Glyph(path: q, color: c, letter: ch))
            x += adv.width + tracking
        }
        for g in line {
            let q = moved(g.path, 512 - lb.midX, 0)
            box = box.union(q.boundingBoxOfPath)
            out.append(Glyph(path: q, color: g.color, letter: g.letter))
        }
    }
    let dy = r.pix(centreY - box.midY)
    return out.map { Glyph(path: moved($0.path, r.small ? r.pix($0.path.boundingBoxOfPath.minX) - $0.path.boundingBoxOfPath.minX : 0, dy), color: $0.color, letter: $0.letter) }
}

/// One glyph whose ink is centred on `c`.
func centred(_ ch: String, _ f: CTFont, _ color: CGColor, at c: CGPoint, _ r: R) -> Glyph {
    let p = glyphPath(ch, f), b = p.boundingBoxOfPath
    return Glyph(path: moved(p, r.pix(c.x - b.midX), r.pix(c.y - b.midY)), color: color, letter: ch)
}

func fill(_ gs: [Glyph], _ r: R) {
    for g in gs { r.ctx.addPath(g.path); r.ctx.setFillColor(g.color); r.ctx.fillPath() }
}

/// Rounded rect with its own radius per corner (tl, tr, br, bl).
func roundRect(_ rc: CGRect, _ tl: CGFloat, _ tr: CGFloat, _ br: CGFloat, _ bl: CGFloat) -> CGPath {
    let p = CGMutablePath()
    p.move(to: CGPoint(x: rc.minX + tl, y: rc.maxY))
    p.addArc(tangent1End: CGPoint(x: rc.maxX, y: rc.maxY), tangent2End: CGPoint(x: rc.maxX, y: rc.minY), radius: tr)
    p.addArc(tangent1End: CGPoint(x: rc.maxX, y: rc.minY), tangent2End: CGPoint(x: rc.minX, y: rc.minY), radius: br)
    p.addArc(tangent1End: CGPoint(x: rc.minX, y: rc.minY), tangent2End: CGPoint(x: rc.minX, y: rc.maxY), radius: bl)
    p.addArc(tangent1End: CGPoint(x: rc.minX, y: rc.maxY), tangent2End: CGPoint(x: rc.maxX, y: rc.maxY), radius: tl)
    p.closeSubpath()
    return p
}

func roundRect(_ rc: CGRect, _ rad: CGFloat) -> CGPath { roundRect(rc, rad, rad, rad, rad) }

func vGradient(_ r: R, _ top: CGColor, _ bottom: CGColor, _ rc: CGRect) {
    let g = CGGradient(colorsSpace: CGColorSpace(name: CGColorSpace.sRGB)!, colors: [top, bottom] as CFArray, locations: [0, 1])!
    r.ctx.drawLinearGradient(g, start: CGPoint(x: 0, y: rc.maxY), end: CGPoint(x: 0, y: rc.minY),
                             options: [.drawsBeforeStartLocation, .drawsAfterEndLocation])
}

/// The macOS tile: drop shadow, vertical gradient, hairline edge. Leaves the context clipped
/// to the tile (balance with r.ctx.restoreGState()).
func beginTile(_ r: R, _ top: CGColor, _ bottom: CGColor, edge: CGColor, glass: Bool = false) {
    let ctx = r.ctx, path = tilePath(tileRect)
    ctx.saveGState()
    if !r.small { ctx.setShadow(offset: CGSize(width: 0, height: -12 * r.scale), blur: 28 * r.scale, color: rgb(0, 0.3)) }
    ctx.addPath(path); ctx.setFillColor(bottom); ctx.fillPath()
    ctx.restoreGState()
    ctx.saveGState()
    ctx.addPath(path); ctx.clip()
    vGradient(r, top, bottom, tileRect)
    ctx.addPath(path); ctx.setStrokeColor(edge); ctx.setLineWidth(r.line(8, minPx: 2)); ctx.strokePath()
    if glass && !r.small {
        // macOS 26 "Liquid Glass" rim: a bright inner edge at the top that fades downwards.
        ctx.saveGState()
        ctx.addPath(path); ctx.setLineWidth(10); ctx.replacePathWithStrokedPath(); ctx.clip()
        vGradient(r, rgb(0xffffff, 0.55), rgb(0xffffff, 0.0), CGRect(x: 0, y: 512, width: 1024, height: 412))
        ctx.restoreGState()
        ctx.saveGState()
        ctx.addPath(path); ctx.setLineWidth(10); ctx.replacePathWithStrokedPath(); ctx.clip()
        vGradient(r, rgb(0xffffff, 0.0), rgb(0xffffff, 0.18), CGRect(x: 0, y: 100, width: 1024, height: 300))
        ctx.restoreGState()
    }
}

/// The 2x2 pane rects inside the tile, with `gap` between them and `inset` from the tile edge.
func panes(inset: CGFloat, gap: CGFloat, _ r: R) -> [CGRect] {
    let a = tileRect.insetBy(dx: inset, dy: inset)
    let mid = CGPoint(x: 512, y: 512)
    let g = r.small ? max(gap, r.px) : gap
    var rects = [
        CGRect(x: a.minX, y: mid.y + g / 2, width: mid.x - g / 2 - a.minX, height: a.maxY - mid.y - g / 2),  // tl
        CGRect(x: mid.x + g / 2, y: mid.y + g / 2, width: a.maxX - mid.x - g / 2, height: a.maxY - mid.y - g / 2),  // tr
        CGRect(x: a.minX, y: a.minY, width: mid.x - g / 2 - a.minX, height: mid.y - g / 2 - a.minY),  // bl
        CGRect(x: mid.x + g / 2, y: a.minY, width: a.maxX - mid.x - g / 2, height: mid.y - g / 2 - a.minY),  // br
    ]
    if r.small { rects = rects.map { r.pix($0) } }
    return rects
}

/// Pane outline whose outer corner follows the tile's corner and inner corners are tight.
func panePath(_ rc: CGRect, index i: Int, outer: CGFloat, inner: CGFloat) -> CGPath {
    switch i {
    case 0: return roundRect(rc, outer, inner, inner, inner)
    case 1: return roundRect(rc, inner, outer, inner, inner)
    case 2: return roundRect(rc, inner, inner, inner, outer)
    default: return roundRect(rc, inner, inner, outer, inner)
    }
}

let kmux = (dark: [("k", dark.k), ("m", dark.m), ("u", dark.u), ("x", dark.x)],
            light: [("k", light.k), ("m", light.m), ("u", light.u), ("x", light.x)])

// v01: two tmux panes side by side: k over m on the left, u over x on the right,
// the bar as the pane divider down the middle.
func v01(_ r: R) {
    let ctx = r.ctx
    beginTile(r, rgb(0x22262d), rgb(0x14161a), edge: rgb(0xffffff, 0.10))
    // Right pane a shade lighter, like an inactive vs active pane.
    ctx.setFillColor(rgb(0xffffff, 0.035)); ctx.fill(CGRect(x: 512, y: 0, width: 512, height: 1024))
    let bw = r.small ? r.px * (r.tiny ? 1 : 2) : 10
    ctx.setFillColor(dark.bar); ctx.fill(r.pix(CGRect(x: 512 - bw / 2, y: 100, width: bw, height: 824)))
    let f = sysFont(r.small ? 400 : 330, r.small ? .heavy : .bold, .monospaced)
    let L = kmux.dark
    let gs = grid([[L[0], L[2]], [L[1], L[3]]], f, cols: [312, 712], lead: r.small ? 360 : 320,
                  centre: CGPoint(x: 512, y: 512), r)
    fill(gs, r)
    ctx.restoreGState()
}

// v02: 2x2 grid of panes, split lines as gaps, one letter per pane.
func v02(_ r: R) {
    let ctx = r.ctx
    beginTile(r, rgb(0x2c313a), rgb(0x1b1e24), edge: rgb(0xffffff, 0.10))
    let ps = panes(inset: r.small ? 0 : 44, gap: r.small ? r.px : 26, r)
    for (i, rc) in ps.enumerated() {
        ctx.saveGState()
        ctx.addPath(panePath(rc, index: i, outer: r.small ? 0 : 150, inner: r.small ? 0 : 36)); ctx.clip()
        vGradient(r, rgb(0x1b1e24), rgb(0x0f1114), rc)
        ctx.restoreGState()
    }
    let f = sysFont(r.small ? 380 : 300, r.small ? .heavy : .bold, .monospaced)
    let L = kmux.dark
    // Each row shares a baseline (k's ascender rises above m), centred in its pane row.
    let gs = grid([[L[0], L[1]]], f, cols: [ps[0].midX, ps[1].midX], lead: 0, centre: CGPoint(x: 512, y: ps[0].midY), r)
        + grid([[L[2], L[3]]], f, cols: [ps[2].midX, ps[3].midX], lead: 0, centre: CGPoint(x: 512, y: ps[2].midY), r)
    fill(gs, r)
    ctx.restoreGState()
}

// v03: four coloured pane tiles, white letters.
func v03(_ r: R) {
    let ctx = r.ctx
    beginTile(r, rgb(0x22262d), rgb(0x14161a), edge: rgb(0xffffff, 0.10))
    let ps = panes(inset: r.small ? 0 : 52, gap: r.small ? r.px : 30, r)
    let f = sysFont(r.small ? 400 : 330, .heavy, .rounded)
    for (i, (_, c)) in kmux.dark.enumerated() {
        let rc = ps[i]
        ctx.saveGState()
        ctx.addPath(panePath(rc, index: i, outer: r.small ? 0 : 140, inner: r.small ? 0 : 44)); ctx.clip()
        ctx.setFillColor(c); ctx.fill(rc)
        if !r.small {   // soft top-light so the panes read as tiles
            vGradient(r, rgb(0xffffff, 0.22), rgb(0x000000, 0.10), rc)
        }
        ctx.restoreGState()
        if r.tiny { continue }   // 16 px: four colour squares are the icon
        // Shared baseline per row: centre the row's ink, not each letter's.
        let row = i < 2 ? [kmux.dark[0], kmux.dark[1]] : [kmux.dark[2], kmux.dark[3]]
        let pair = grid([row.map { ($0.0, rgb(0xffffff)) }], f, cols: [ps[i < 2 ? 0 : 2].midX, ps[i < 2 ? 1 : 3].midX],
                        lead: 0, centre: CGPoint(x: 512, y: rc.midY), r)
        let g = pair[i % 2]
        ctx.saveGState()
        if !r.small { ctx.setShadow(offset: CGSize(width: 0, height: -6 * r.scale), blur: 14 * r.scale, color: rgb(0, 0.25)) }
        ctx.addPath(g.path); ctx.setFillColor(rgb(0xffffff)); ctx.fillPath()
        ctx.restoreGState()
    }
    ctx.restoreGState()
}

// v04: a terminal window: light window chrome, dark screen, km over ux split by a pane
// line, and a green tmux status line along the bottom.
func v04(_ r: R) {
    let ctx = r.ctx
    beginTile(r, rgb(0xffffff), rgb(0xe3e6eb), edge: rgb(0x000000, 0.12))
    let titleH: CGFloat = r.small ? 96 : 120
    let screen = r.pix(CGRect(x: r.small ? 160 : 150, y: r.small ? 160 : 150,
                              width: r.small ? 704 : 724, height: 824 - titleH - (r.small ? 60 : 50)))
    if !r.small {
        for (i, c) in [0xd8dce2, 0xd8dce2, 0xd8dce2].enumerated() {
            ctx.setFillColor(rgb(UInt32(c)))
            ctx.fillEllipse(in: CGRect(x: 196 + CGFloat(i) * 64, y: 924 - titleH / 2 - 20, width: 40, height: 40))
        }
    }
    ctx.saveGState()
    ctx.addPath(roundRect(screen, r.small ? 40 : 64)); ctx.clip()
    vGradient(r, rgb(0x1b1e24), rgb(0x0f1114), screen)
    // tmux status line
    let sh: CGFloat = r.small ? max(64, 2 * r.px) : 64
    ctx.setFillColor(ok.dark); ctx.fill(r.pix(CGRect(x: screen.minX, y: screen.minY, width: screen.width, height: sh)))
    let area = CGRect(x: screen.minX, y: screen.minY + sh, width: screen.width, height: screen.height - sh)
    // Pane split between the rows
    let lw = r.small ? r.px : 8
    if !r.tiny {
    ctx.setFillColor(rgb(0x8b93a0)); ctx.fill(r.pix(CGRect(x: area.minX, y: area.midY - lw / 2, width: area.width, height: lw)))
    }
    let f = sysFont(r.small ? 330 : 270, r.small ? .heavy : .bold, .monospaced)
    let L = kmux.dark
    let top = grid([[L[0], L[1]]], f, cols: [area.midX - 0.2 * area.width, area.midX + 0.2 * area.width], lead: 0,
                   centre: CGPoint(x: area.midX, y: area.midY + area.height / 4), r)
    let bot = grid([[L[2], L[3]]], f, cols: [area.midX - 0.2 * area.width, area.midX + 0.2 * area.width], lead: 0,
                   centre: CGPoint(x: area.midX, y: area.midY - area.height / 4), r)
    fill(top + bot, r)
    ctx.restoreGState()
    ctx.restoreGState()
}

// v05: bold monogram k|x; the bar carries the two middle letters, red over purple.
func v05(_ r: R) {
    let ctx = r.ctx
    beginTile(r, rgb(0x22262d), rgb(0x14161a), edge: rgb(0xffffff, 0.10))
    let f = sysFont(r.small ? 560 : 500, r.small ? .heavy : .bold, .monospaced)
    let k = glyphPath("k", f), x = glyphPath("x", f)
    let kb = k.boundingBoxOfPath, xb = x.boundingBoxOfPath
    let barW: CGFloat = r.small ? 3 * r.px : 64, gap: CGFloat = r.small ? 1.5 * r.px : 44
    let total = kb.width + gap + barW + gap + xb.width
    let x0 = 512 - total / 2
    let base = r.pix(512 - kb.height / 2 + (r.small ? 0 : 0))
    let kg = moved(k, r.pix(x0 - kb.minX), base - kb.minY)
    let xg = moved(x, r.pix(x0 + kb.width + gap + barW + gap - xb.minX), base - xb.minY)
    fill([Glyph(path: kg, color: dark.k, letter: "k"), Glyph(path: xg, color: dark.x, letter: "x")], r)
    // Bar: from below the baseline to the k's ascender, top half red, bottom half purple.
    let bar = r.pix(CGRect(x: x0 + kb.width + gap, y: base - (r.small ? 0 : 40), width: barW, height: kb.height + (r.small ? 0 : 80)))
    ctx.saveGState()
    ctx.addPath(roundRect(bar, r.small ? 0 : barW / 2)); ctx.clip()
    if r.small {
        ctx.setFillColor(dark.m); ctx.fill(CGRect(x: bar.minX, y: r.pix(bar.midY), width: bar.width, height: bar.maxY - r.pix(bar.midY)))
        ctx.setFillColor(dark.u); ctx.fill(CGRect(x: bar.minX, y: bar.minY, width: bar.width, height: r.pix(bar.midY) - bar.minY))
    } else {
        vGradient(r, dark.m, dark.u, bar)
    }
    ctx.restoreGState()
    ctx.restoreGState()
}

/// Stacked "km" over "ux" with a horizontal bar between the rows; shared by v06, v07, v09.
func stack(_ r: R, _ f: CTFont, _ L: [(String, CGColor)], colGap: CGFloat, lead: CGFloat, tracking: CGFloat? = nil) -> (glyphs: [Glyph], barY: CGFloat, left: CGFloat, right: CGFloat) {
    let gs = tracking.map { words([[L[0], L[1]], [L[2], L[3]]], f, tracking: $0, lead: lead, centreY: 512, r) }
        ?? grid([[L[0], L[1]], [L[2], L[3]]], f, cols: [512 - colGap / 2, 512 + colGap / 2], lead: lead,
                centre: CGPoint(x: 512, y: 512), r)
    // The bar sits midway between the bottom of the top row (baseline) and the x-height of the second.
    let m = gs[1].path.boundingBoxOfPath, u = gs[2].path.boundingBoxOfPath
    let all = gs.reduce(CGRect.null) { $0.union($1.path.boundingBoxOfPath) }
    return (gs, (m.minY + u.maxY) / 2, all.minX, all.maxX)
}

// v06: light: white tile, SF Rounded, light-mode letter colours, green pill bar.
func v06(_ r: R) {
    let ctx = r.ctx
    beginTile(r, rgb(0xffffff), rgb(0xe9ecf0), edge: rgb(0x000000, 0.10))
    let f = sysFont(r.small ? 400 : 340, r.small ? .black : .heavy, .rounded)
    let s = stack(r, f, kmux.light, colGap: 0, lead: r.small ? 370 : 330, tracking: r.small ? 30 : 16)
    fill(s.glyphs, r)
    let h: CGFloat = r.small ? 2 * r.px : 30
    let bar = r.pix(CGRect(x: s.left, y: s.barY - h / 2, width: s.right - s.left, height: h))
    ctx.addPath(roundRect(bar, r.small ? 0 : h / 2)); ctx.setFillColor(ok.light); ctx.fillPath()
    ctx.restoreGState()
}

// v07: outlined letters on near-black; filled again below 64 px where outlines would close up.
func v07(_ r: R) {
    let ctx = r.ctx
    beginTile(r, rgb(0x1b1e24), rgb(0x0c0d10), edge: rgb(0xffffff, 0.10))
    let f = sysFont(r.small ? 420 : 360, r.small ? .heavy : .bold, .monospaced)
    let s = stack(r, f, kmux.dark, colGap: r.small ? 250 : 228, lead: r.small ? 360 : 330)
    if r.size < 64 {
        fill(s.glyphs, r)
    } else {
        for g in s.glyphs {
            ctx.addPath(g.path); ctx.setFillColor(g.color.copy(alpha: 0.14)!); ctx.fillPath()
            ctx.addPath(g.path); ctx.setStrokeColor(g.color); ctx.setLineWidth(r.line(18, minPx: 1.2))
            ctx.setLineJoin(.round); ctx.strokePath()
        }
    }
    let h: CGFloat = r.small ? r.px * (r.tiny ? 1 : 2) : 18
    ctx.setFillColor(dark.bar); ctx.fill(r.pix(CGRect(x: s.left - 6, y: s.barY - h / 2, width: s.right - s.left + 12, height: h)))
    ctx.restoreGState()
}

// v08: macOS 26 glass: indigo-to-ink gradient, two frosted panes (km, ux), glowing letters.
func v08(_ r: R) {
    let ctx = r.ctx
    beginTile(r, rgb(0x3b2f6b), rgb(0x10142a), edge: rgb(0xffffff, 0.14), glass: true)
    if !r.small {   // colour wash from the letter hues behind the glass
        let cs = CGColorSpace(name: CGColorSpace.sRGB)!
        for (c, p) in [(dark.u, CGPoint(x: 260, y: 820)), (dark.x, CGPoint(x: 820, y: 220)), (dark.m, CGPoint(x: 860, y: 860))] {
            let g = CGGradient(colorsSpace: cs, colors: [c.copy(alpha: 0.35)!, c.copy(alpha: 0)!] as CFArray, locations: [0, 1])!
            ctx.drawRadialGradient(g, startCenter: p, startRadius: 0, endCenter: p, endRadius: 520, options: [])
        }
    }
    let inset: CGFloat = r.small ? 40 : 70, gap: CGFloat = r.small ? 2 * r.px : 34
    let a = tileRect.insetBy(dx: inset, dy: inset)
    let topP = r.pix(CGRect(x: a.minX, y: 512 + gap / 2, width: a.width, height: a.maxY - 512 - gap / 2))
    let botP = r.pix(CGRect(x: a.minX, y: a.minY, width: a.width, height: 512 - gap / 2 - a.minY))
    for (i, p) in [topP, botP].enumerated() {
        let path = i == 0 ? roundRect(p, 130, 130, 40, 40) : roundRect(p, 40, 40, 130, 130)
        ctx.saveGState()
        if !r.small { ctx.setShadow(offset: CGSize(width: 0, height: -10 * r.scale), blur: 30 * r.scale, color: rgb(0, 0.35)) }
        ctx.addPath(path); ctx.setFillColor(rgb(0xffffff, r.small ? 0.10 : 0.09)); ctx.fillPath()
        ctx.restoreGState()
        if !r.small {
            ctx.saveGState()
            ctx.addPath(path); ctx.clip()
            vGradient(r, rgb(0xffffff, 0.10), rgb(0xffffff, 0), CGRect(x: 0, y: p.midY, width: 1024, height: p.height / 2))
            ctx.addPath(path); ctx.setLineWidth(6); ctx.replacePathWithStrokedPath(); ctx.clip()
            vGradient(r, rgb(0xffffff, 0.6), rgb(0xffffff, 0.08), p)
            ctx.restoreGState()
        }
    }
    let f = sysFont(r.small ? 380 : 320, r.small ? .heavy : .bold, .rounded)
    let L = kmux.dark
    let top = grid([[L[0], L[1]]], f, cols: [512 - 125, 512 + 125], lead: 0, centre: CGPoint(x: 512, y: topP.midY), r)
    let bot = grid([[L[2], L[3]]], f, cols: [512 - 125, 512 + 125], lead: 0, centre: CGPoint(x: 512, y: botP.midY), r)
    for g in top + bot {
        ctx.saveGState()
        if !r.small { ctx.setShadow(offset: .zero, blur: 40 * r.scale, color: g.color.copy(alpha: 0.75)!) }
        ctx.addPath(g.path); ctx.setFillColor(g.color); ctx.fillPath()
        ctx.restoreGState()
        if !r.small {   // light from above
            ctx.saveGState()
            ctx.addPath(g.path); ctx.clip()
            let b = g.path.boundingBoxOfPath
            vGradient(r, rgb(0xffffff, 0.35), rgb(0xffffff, 0), CGRect(x: b.minX, y: b.midY, width: b.width, height: b.height / 2))
            ctx.restoreGState()
        }
    }
    ctx.restoreGState()
}

// v09: serif (New York black) on warm ink, green bar: editorial rather than terminal.
func v09(_ r: R) {
    let ctx = r.ctx
    beginTile(r, rgb(0x262329), rgb(0x141316), edge: rgb(0xffffff, 0.10))
    let f = sysFont(r.small ? 440 : 380, r.small ? .black : .heavy, .serif)
    let s = stack(r, f, kmux.dark, colGap: 0, lead: r.small ? 360 : 330, tracking: r.small ? 40 : 18)
    fill(s.glyphs, r)
    let h: CGFloat = r.small ? 2 * r.px : 26
    let bar = r.pix(CGRect(x: s.left, y: s.barY - h / 2, width: s.right - s.left, height: h))
    ctx.addPath(roundRect(bar, r.small ? 0 : h / 2)); ctx.setFillColor(ok.dark); ctx.fillPath()
    ctx.restoreGState()
}

// v10: Kanna's sibling: black tile, k m over u x, and a tall green cursor pill as the "|"
// between the columns (Kanna.app's icon has the same green cursor-like pill).
func v10(_ r: R) {
    let ctx = r.ctx
    beginTile(r, rgb(0x1b1e24), rgb(0x08090b), edge: rgb(0xffffff, 0.12), glass: true)
    let f = sysFont(r.small ? 400 : 330, r.small ? .heavy : .bold, .monospaced)
    let L = kmux.dark
    let colGap: CGFloat = r.small ? 330 : 310
    let gs = grid([[L[0], L[1]], [L[2], L[3]]], f, cols: [512 - colGap / 2, 512 + colGap / 2], lead: r.small ? 360 : 330,
                  centre: CGPoint(x: 512, y: 512), r)
    fill(gs, r)
    let all = gs.reduce(CGRect.null) { $0.union($1.path.boundingBoxOfPath) }
    let w: CGFloat = r.small ? 2 * r.px : 44
    let pill = r.pix(CGRect(x: 512 - w / 2, y: all.minY, width: w, height: all.height))
    ctx.saveGState()
    if !r.small { ctx.setShadow(offset: .zero, blur: 36 * r.scale, color: ok.dark.copy(alpha: 0.6)!) }
    ctx.addPath(roundRect(pill, r.small ? 0 : w / 2)); ctx.setFillColor(ok.dark); ctx.fillPath()
    ctx.restoreGState()
    ctx.restoreGState()
}

// MARK: Explorations v11-v16: Kanna.app's own icon palette
//
// Kanna.app's icon: a near-white tile with rounded pills in smooth left-to-right gradients
// (pink-red -> orange, magenta, purple, indigo -> violet, blue) and a bright green cursor pill.
// Each letter gets one of those gradients: k orange, m magenta, u purple, x blue; the bar is
// the green cursor. At the smallest sizes the letters go and four quadrants in the same
// gradients remain: a 2x2 pane grid.

struct Grad { let a: CGColor, b: CGColor }
let kanna = (k: Grad(a: rgb(0xfe6574), b: rgb(0xff9c1b)),     // top row: hot pink-red -> orange
             m: Grad(a: rgb(0xec45b1), b: rgb(0xf54aa2)),     // magenta -> pink
             u: Grad(a: rgb(0xb44ae2), b: rgb(0x7f5db4)),     // purple -> violet
             x: Grad(a: rgb(0x1665c3), b: rgb(0x3f58c2)),     // blue -> indigo
             cursor: rgb(0x00df65),
             tileTop: rgb(0xfffeff), tileBottom: rgb(0xf4f4f5), rim: rgb(0x000000, 0.09))
let kannaGrads = [kanna.k, kanna.m, kanna.u, kanna.x]

/// Largest size (px) drawn as four quadrants instead of letters. The sheet renders 32 px both
/// ways; 16 is where letters stop being letters, so that is the default switch.
var quadMax = 16

/// Fills `path` with a left-to-right gradient across its bounds (Kanna's pills run this way).
func fillGrad(_ r: R, _ path: CGPath, _ g: Grad) {
    let ctx = r.ctx, b = path.boundingBoxOfPath
    ctx.saveGState()
    ctx.addPath(path); ctx.clip()
    let cg = CGGradient(colorsSpace: CGColorSpace(name: CGColorSpace.sRGB)!, colors: [g.a, g.b] as CFArray, locations: [0, 1])!
    ctx.drawLinearGradient(cg, start: CGPoint(x: b.minX, y: b.midY), end: CGPoint(x: b.maxX, y: b.midY),
                           options: [.drawsBeforeStartLocation, .drawsAfterEndLocation])
    ctx.restoreGState()
}

func kannaTile(_ r: R, darkTile: Bool) {
    if darkTile { beginTile(r, rgb(0x24232a), rgb(0x111015), edge: rgb(0xffffff, 0.12)) }
    else { beginTile(r, kanna.tileTop, kanna.tileBottom, edge: kanna.rim) }
}

/// The small-size form: four quadrants, one letter's gradient each, on the tile.
/// Returns false when the size is large enough for letters.
func quadrants(_ r: R, darkTile: Bool) -> Bool {
    guard r.size <= quadMax else { return false }
    kannaTile(r, darkTile: darkTile)
    // Whole-pixel geometry: tile edge, a 1 px tile border and a 1 px cross between panes.
    let t = r.pix(tileRect)
    let b = r.px, g = r.px
    let cx = r.pix(512 - g / 2)
    let rects = [
        CGRect(x: t.minX + b, y: cx + g, width: cx - t.minX - b, height: t.maxY - b - cx - g),
        CGRect(x: cx + g, y: cx + g, width: t.maxX - b - cx - g, height: t.maxY - b - cx - g),
        CGRect(x: t.minX + b, y: t.minY + b, width: cx - t.minX - b, height: cx - t.minY - b),
        CGRect(x: cx + g, y: t.minY + b, width: t.maxX - b - cx - g, height: cx - t.minY - b),
    ]
    let rad = r.size <= 16 ? 0 : r.px   // a hint of roundness once there are pixels to spend
    for (i, rc) in rects.enumerated() {
        fillGrad(r, panePath(rc, index: i, outer: rad * 5, inner: rad), kannaGrads[i])
    }
    r.ctx.restoreGState()
    return true
}

func roundedHeavy(_ size: CGFloat) -> CTFont { sysFont(size, .heavy, .rounded) }

/// Letters k m / u x set as words in SF Rounded, each filled with its gradient.
func kannaWords(_ r: R, size: CGFloat, tracking: CGFloat, lead: CGFloat, centreY: CGFloat = 512) -> [Glyph] {
    let c = rgb(0)   // colour unused: glyphs are gradient-filled
    return words([[("k", c), ("m", c)], [("u", c), ("x", c)]], roundedHeavy(size), tracking: tracking, lead: lead, centreY: centreY, r)
}

func fillKanna(_ r: R, _ gs: [Glyph]) {
    for (i, g) in gs.enumerated() { fillGrad(r, g.path, kannaGrads[i]) }
}

func cursorPill(_ r: R, _ rc: CGRect, glow: Bool = false) {
    let ctx = r.ctx
    ctx.saveGState()
    if glow { ctx.setShadow(offset: .zero, blur: 30 * r.scale, color: kanna.cursor.copy(alpha: 0.55)!) }
    ctx.addPath(roundRect(rc, min(rc.width, rc.height) / 2)); ctx.setFillColor(kanna.cursor); ctx.fillPath()
    ctx.restoreGState()
}

/// "km" + cursor over "ux", left-aligned like Kanna's rows of pills.
func kannaRows(_ r: R, darkTile: Bool) {
    if quadrants(r, darkTile: darkTile) { return }
    kannaTile(r, darkTile: darkTile)
    var gs = kannaWords(r, size: r.small ? 360 : 310, tracking: r.small ? 20 : 8, lead: r.small ? 330 : 290)
    let top = gs[0].path.boundingBoxOfPath.union(gs[1].path.boundingBoxOfPath)
    let bot = gs[2].path.boundingBoxOfPath.union(gs[3].path.boundingBoxOfPath)
    let mb = gs[1].path.boundingBoxOfPath
    let pillW = mb.height * (r.small ? 0.5 : 0.40), pillGap = mb.height * 0.28
    let width = top.width + pillGap + pillW
    let dx = (512 - width / 2) - top.minX
    gs = gs.enumerated().map { i, g in
        Glyph(path: moved(g.path, dx + (i >= 2 ? top.minX - bot.minX : 0), 0), color: g.color, letter: g.letter)
    }
    fillKanna(r, gs)
    let m = gs[1].path.boundingBoxOfPath
    cursorPill(r, r.pix(CGRect(x: m.maxX + pillGap, y: m.minY, width: pillW, height: m.height)), glow: darkTile && !r.small)
    r.ctx.restoreGState()
}

// v11: Kanna rows on Kanna's light tile: "km" + green cursor, "ux" below, left-aligned.
func v11(_ r: R) { kannaRows(r, darkTile: false) }

// v12: centred stack, the green cursor pill laid flat as the bar between km and ux.
func v12(_ r: R) { v12Font(r, roundedHeavy, tracking: 10) }

/// v12 in another font. The font is sized so its x-height matches SF Rounded Heavy at the
/// original v12 size, so every font alternative has the same optical size; `tracking` is in
/// 1024 units at large sizes (small sizes add their own).
///
/// `measured` sizes by the ink height of "x" instead of the font's x-height metric (font files
/// from elsewhere don't always have a trustworthy one) and shrinks the font if "km" would run
/// wider than v12's safe area.
func v12Font(_ r: R, _ make: (CGFloat) -> CTFont, tracking: CGFloat, scale: CGFloat = 1, measured: Bool = false) {
    if quadrants(r, darkTile: false) { return }
    kannaTile(r, darkTile: false)
    let base: CGFloat = r.small ? 400 : 330
    func xh(_ f: CTFont) -> CGFloat {
        let ink = glyphPath("x", f).boundingBoxOfPath.height
        return measured && ink > 0 && ink.isFinite ? ink : CTFontGetXHeight(f)
    }
    let target = xh(roundedHeavy(base)) * scale
    var size = base * target / xh(make(base))
    if measured {
        // Matching x-height alone leaves narrow fonts looking small and wide ones big: meet
        // halfway (geometric mean) between the x-height match and a match of "km"'s width.
        func kmWidth(_ f: CTFont, _ t: CGFloat) -> CGFloat {
            let km = words([[("k", rgb(0)), ("m", rgb(0))]], f, tracking: t, lead: 0, centreY: 512, r)
            return km[0].path.boundingBoxOfPath.union(km[1].path.boundingBoxOfPath).width
        }
        let w = kmWidth(make(size), tracking), want = kmWidth(roundedHeavy(base), 10)
        if w > 0 { size *= (want / w).squareRoot() }
        let w2 = kmWidth(make(size), tracking)
        if w2 > 640 { size *= 640 / w2 }
    }
    let f = make(size)
    let c = rgb(0)
    let gs = words([[("k", c), ("m", c)], [("u", c), ("x", c)]], f, tracking: tracking + (r.small ? 14 : 0),
                   lead: r.small ? 360 : 310, centreY: 512, r)
    fillKanna(r, gs)
    let top = gs[0].path.boundingBoxOfPath.union(gs[1].path.boundingBoxOfPath)
    let u = gs[2].path.boundingBoxOfPath
    let h: CGFloat = r.small ? 2 * r.px : 36
    cursorPill(r, r.pix(CGRect(x: top.minX + 20, y: (top.minY + u.maxY) / 2 - h / 2, width: top.width - 40, height: h)))
    r.ctx.restoreGState()
}

func sfPro(_ w: NSFont.Weight) -> (CGFloat) -> CTFont { { sysFont($0, w, .default) } }
// v12a-e: v12's font alternatives (crisper than SF Rounded).
func v12a(_ r: R) { v12Font(r, sfPro(.bold), tracking: 6) }
func v12b(_ r: R) { v12Font(r, sfPro(.heavy), tracking: 6) }
func v12c(_ r: R) { v12Font(r, sfPro(.semibold), tracking: 22, scale: 1.02) }
func v12d(_ r: R) { v12Font(r, { sysFont($0, .bold, .monospaced) }, tracking: -10) }
func v12e(_ r: R) { v12Font(r, { NSFont.systemFont(ofSize: $0, weight: .black, width: .condensed) as CTFont }, tracking: 14, scale: 1.04) }

// v13: four gradient pane tiles with white letters: the quadrant form, lettered.
func v13(_ r: R) {
    if quadrants(r, darkTile: false) { return }
    kannaTile(r, darkTile: false)
    let ctx = r.ctx
    let ps = panes(inset: r.small ? 24 : 46, gap: r.small ? r.px : 28, r)
    let f = roundedHeavy(r.small ? 400 : 320)
    let rows = [[("k", rgb(0xffffff)), ("m", rgb(0xffffff))], [("u", rgb(0xffffff)), ("x", rgb(0xffffff))]]
    let gs = grid([rows[0]], f, cols: [ps[0].midX, ps[1].midX], lead: 0, centre: CGPoint(x: 512, y: ps[0].midY), r)
        + grid([rows[1]], f, cols: [ps[2].midX, ps[3].midX], lead: 0, centre: CGPoint(x: 512, y: ps[2].midY), r)
    for (i, rc) in ps.enumerated() {
        fillGrad(r, panePath(rc, index: i, outer: r.small ? r.px * 2 : 140, inner: r.small ? r.px : 44), kannaGrads[i])
        ctx.saveGState()
        if !r.small { ctx.setShadow(offset: CGSize(width: 0, height: -5 * r.scale), blur: 12 * r.scale, color: rgb(0, 0.18)) }
        ctx.addPath(gs[i].path); ctx.setFillColor(rgb(0xffffff)); ctx.fillPath()
        ctx.restoreGState()
    }
    ctx.restoreGState()
}

// v14: k m over u x on the light tile, a tall green cursor between the columns.
func v14(_ r: R) {
    if quadrants(r, darkTile: false) { return }
    kannaTile(r, darkTile: false)
    let f = roundedHeavy(r.small ? 380 : 290)
    let c = rgb(0), off: CGFloat = r.small ? 200 : 200
    let gs = grid([[("k", c), ("m", c)], [("u", c), ("x", c)]], f, cols: [512 - off, 512 + off],
                  lead: r.small ? 360 : 310, centre: CGPoint(x: 512, y: 512), r)
    fillKanna(r, gs)
    let all = gs.reduce(CGRect.null) { $0.union($1.path.boundingBoxOfPath) }
    let w: CGFloat = r.small ? 2 * r.px : 46
    cursorPill(r, r.pix(CGRect(x: 512 - w / 2, y: all.minY + 20, width: w, height: all.height - 40)))
    r.ctx.restoreGState()
}

// v15: v11 on a dark tile: the one dark alternative, with a glowing cursor.
func v15(_ r: R) { kannaRows(r, darkTile: true) }

// v16: four gradient dots (Kanna's round pills) in a 2x2, white letters, green cursor dot
// tucked in the middle.
func v16(_ r: R) {
    if quadrants(r, darkTile: false) { return }
    kannaTile(r, darkTile: false)
    let ctx = r.ctx
    let d: CGFloat = r.small ? 360 : 312, off: CGFloat = r.small ? 185 : 168
    let centres = [CGPoint(x: 512 - off, y: 512 + off), CGPoint(x: 512 + off, y: 512 + off),
                   CGPoint(x: 512 - off, y: 512 - off), CGPoint(x: 512 + off, y: 512 - off)]
    let f = roundedHeavy(r.small ? 330 : 236)
    let w = rgb(0xffffff)
    let gs = grid([[("k", w), ("m", w)]], f, cols: [centres[0].x, centres[1].x], lead: 0, centre: CGPoint(x: 512, y: centres[0].y), r)
        + grid([[("u", w), ("x", w)]], f, cols: [centres[2].x, centres[3].x], lead: 0, centre: CGPoint(x: 512, y: centres[2].y), r)
    for (i, c) in centres.enumerated() {
        let dot = CGPath(ellipseIn: r.pix(CGRect(x: c.x - d / 2, y: c.y - d / 2, width: d, height: d)), transform: nil)
        fillGrad(r, dot, kannaGrads[i])
        ctx.addPath(gs[i].path); ctx.setFillColor(w); ctx.fillPath()
    }
    if !r.small { cursorPill(r, CGRect(x: 512 - 34, y: 512 - 56, width: 68, height: 112)) }
    ctx.restoreGState()
}

/// Round-two sheet: per variant, 512 / 64 / 32 letters / 32 quadrants / 16 quadrants, on a light
/// and a dark backdrop.
func makeSheet2(_ names: [String], _ url: URL) {
    makeSheet2(names.compactMap { n in variants.first { $0.name == n } }, url)
}

/// Sheet columns: pixel size, and the largest size drawn as quadrants for that column
/// (so 32 can be shown both lettered and as quadrants).
typealias SheetColumn = (size: Int, quadMax: Int)
let defaultColumns: [SheetColumn] = [(512, 16), (64, 16), (32, 16), (32, 32), (16, 16)]

/// Parses "512,128,64,32,32q,16q" (q = drawn as quadrants).
func parseColumns(_ s: String) -> [SheetColumn] {
    s.split(separator: ",").compactMap { t in
        let q = t.hasSuffix("q"), n = Int(q ? t.dropLast() : t[...]) ?? 0
        return n > 0 ? (n, q ? n : min(16, n - 1)) : nil
    }
}

func makeSheet2(_ vs: [Variant], _ url: URL, columns: [SheetColumn] = defaultColumns) {
    let pad: CGFloat = 32, labelH: CGFloat = 56
    let big = CGFloat(columns.map(\.size).max() ?? 512)
    let half = pad + columns.reduce(0) { $0 + CGFloat($1.size) + pad }
    let rowH = labelH + big + 2 * pad
    let width = 2 * half, height = rowH * CGFloat(vs.count)
    let ctx = CGContext(data: nil, width: Int(width), height: Int(height), bitsPerComponent: 8, bytesPerRow: 0,
                        space: CGColorSpace(name: CGColorSpace.sRGB)!, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
    ctx.interpolationQuality = .none
    for (i, v) in vs.enumerated() {
        let y0 = height - rowH * CGFloat(i + 1)
        for (j, bg) in [(rgb(0xececec), rgb(0x1d2026)), (rgb(0x2b2e33), rgb(0xe6e8eb))].enumerated() {
            let x0 = CGFloat(j) * half
            ctx.setFillColor(bg.0); ctx.fill(CGRect(x: x0, y: y0, width: half, height: rowH))
            var x = x0 + pad
            for (s, q) in columns {
                quadMax = q
                let yy = y0 + pad + (big - CGFloat(s)) / 2
                ctx.draw(render(v, size: s), in: CGRect(x: x, y: yy, width: CGFloat(s), height: CGFloat(s)))
                x += CGFloat(s) + pad
            }
            if j == 0 {
                drawText("\(v.name)  \(v.note)", font(28, weight: .semibold), bg.1, at: CGPoint(x: x0 + pad, y: y0 + rowH - labelH + 8), in: ctx)
            } else {
                var lx = x0 + pad
                for (s, q) in columns {   // size labels on the header line, centred over each column
                    let t = "\(s)" + (q >= s ? "q" : "")
                    drawText(t, font(20, weight: .medium), bg.1, at: CGPoint(x: lx + CGFloat(s) / 2 - CGFloat(t.count) * 6,
                                                                            y: y0 + rowH - labelH + 8), in: ctx)
                    lx += CGFloat(s) + pad
                }
            }
        }
        ctx.setFillColor(rgb(0x888888)); ctx.fill(CGRect(x: 0, y: y0, width: width, height: 2))
    }
    quadMax = 16
    writePNG(ctx.makeImage()!, url)
    print("Wrote \(url.path)")
}

/// Family check: Kanna.app's icon beside chosen variants at 512 and 64 px, on light and dark.
func makeFamily(_ kannaPNG: URL, _ names: [String], _ url: URL) {
    guard let src = NSImage(contentsOf: kannaPNG)?.cgImage(forProposedRect: nil, context: nil, hints: nil) else {
        fatalError("can't read \(kannaPNG.path)")
    }
    let vs = names.compactMap { n in variants.first { $0.name == n } }
    let pad: CGFloat = 32, cell: CGFloat = 512, labelH: CGFloat = 48
    let n = CGFloat(vs.count + 1)
    let width = pad + n * (cell + pad), bandH = labelH + cell + pad + 64 + 2 * pad
    let height = 2 * bandH
    let ctx = CGContext(data: nil, width: Int(width), height: Int(height), bitsPerComponent: 8, bytesPerRow: 0,
                        space: CGColorSpace(name: CGColorSpace.sRGB)!, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
    ctx.interpolationQuality = .high
    for (j, bg) in [(rgb(0xececec), rgb(0x1d2026)), (rgb(0x2b2e33), rgb(0xe6e8eb))].enumerated() {
        let y0 = height - bandH * CGFloat(j + 1)
        ctx.setFillColor(bg.0); ctx.fill(CGRect(x: 0, y: y0, width: width, height: bandH))
        for i in 0..<Int(n) {
            let x = pad + CGFloat(i) * (cell + pad)
            let label = i == 0 ? "Kanna.app" : vs[i - 1].name
            if j == 0 { drawText(label, font(28, weight: .semibold), bg.1, at: CGPoint(x: x, y: y0 + bandH - labelH + 8), in: ctx) }
            for (s, yy) in [(cell, y0 + pad + 64 + pad), (64, y0 + pad)] {
                let rc = CGRect(x: x + (s == 64 ? (cell - 64) / 2 : 0), y: yy, width: CGFloat(s), height: CGFloat(s))
                ctx.draw(i == 0 ? src : render(vs[i - 1], size: Int(s)), in: rc)
            }
        }
    }
    writePNG(ctx.makeImage()!, url)
    print("Wrote \(url.path)")
}

let explorations: [Variant] = [
    Variant(name: "v01", palette: dark, layout: .stack, note: "two panes: k/m | u/x, divider bar", draw: v01),
    Variant(name: "v02", palette: dark, layout: .stack, note: "2x2 panes, split lines", draw: v02),
    Variant(name: "v03", palette: dark, layout: .stack, note: "coloured pane tiles, white letters", draw: v03),
    Variant(name: "v04", palette: dark, layout: .stack, note: "terminal window, tmux status line", draw: v04),
    Variant(name: "v05", palette: dark, layout: .row, note: "monogram k|x, red/purple bar", draw: v05),
    Variant(name: "v06", palette: light, layout: .stack, note: "light, SF Rounded, green bar", draw: v06),
    Variant(name: "v07", palette: dark, layout: .stack, note: "outlined letters", draw: v07),
    Variant(name: "v08", palette: dark, layout: .stack, note: "macOS 26 glass panes", draw: v08),
    Variant(name: "v09", palette: dark, layout: .stack, note: "serif (New York), green bar", draw: v09),
    Variant(name: "v10", palette: dark, layout: .stack, note: "green cursor pill between columns", draw: v10),
    Variant(name: "v11", palette: light, layout: .stack, note: "Kanna rows: km + green cursor / ux", draw: v11),
    Variant(name: "v12", palette: light, layout: .stack, note: "centred km / ux, flat green cursor bar", draw: v12),
    Variant(name: "v12a", palette: light, layout: .stack, note: "v12 in SF Pro Bold", draw: v12a),
    Variant(name: "v12b", palette: light, layout: .stack, note: "v12 in SF Pro Heavy", draw: v12b),
    Variant(name: "v12c", palette: light, layout: .stack, note: "v12 in SF Pro Semibold, wider tracking", draw: v12c),
    Variant(name: "v12d", palette: light, layout: .stack, note: "v12 in SF Mono Bold", draw: v12d),
    Variant(name: "v12e", palette: light, layout: .stack, note: "v12 in SF Pro Condensed Black", draw: v12e),
    Variant(name: "v13", palette: light, layout: .stack, note: "gradient pane tiles, white letters", draw: v13),
    Variant(name: "v14", palette: light, layout: .stack, note: "k m / u x, tall green cursor between", draw: v14),
    Variant(name: "v15", palette: dark, layout: .stack, note: "Kanna rows on a dark tile", draw: v15),
    Variant(name: "v16", palette: light, layout: .stack, note: "gradient dots, white letters", draw: v16),
]

/// Contact sheet: one row per variant, its number, then 512/64/32/16 px at actual size on a
/// light (Finder) and a dark (Dock) backdrop.
func makeSheet(_ names: [String], _ url: URL) {
    let vs = names.compactMap { n in variants.first { $0.name == n } }
    let pad: CGFloat = 36, labelW: CGFloat = 0, labelH: CGFloat = 56
    let half = pad + 512 + pad + 64 + pad + 32 + pad + 16 + pad
    let rowH = labelH + 512 + 2 * pad
    let width = labelW + 2 * half, height = rowH * CGFloat(vs.count)
    let cs = CGColorSpace(name: CGColorSpace.sRGB)!
    let ctx = CGContext(data: nil, width: Int(width), height: Int(height), bitsPerComponent: 8, bytesPerRow: 0,
                        space: cs, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
    ctx.interpolationQuality = .none
    for (i, v) in vs.enumerated() {
        let y0 = height - rowH * CGFloat(i + 1)
        for (j, bg) in [(rgb(0xececec), rgb(0x1d2026)), (rgb(0x2b2e33), rgb(0xe6e8eb))].enumerated() {
            let x0 = labelW + CGFloat(j) * half
            ctx.setFillColor(bg.0); ctx.fill(CGRect(x: x0, y: y0, width: half, height: rowH))
            var x = x0 + pad
            for s in [512, 64, 32, 16] {
                let yy = y0 + pad + (s == 512 ? 0 : 512 / 2 - CGFloat(s) / 2)
                ctx.draw(render(v, size: s), in: CGRect(x: x, y: yy, width: CGFloat(s), height: CGFloat(s)))
                x += CGFloat(s) + pad
            }
            if j == 0 {
                drawText("\(v.name)  \(v.note)", font(30, weight: .semibold), bg.1,
                         at: CGPoint(x: x0 + pad, y: y0 + rowH - labelH + 6), in: ctx)
            }
        }
        ctx.setFillColor(rgb(0x888888)); ctx.fill(CGRect(x: 0, y: y0, width: width, height: 2))
    }
    writePNG(ctx.makeImage()!, url)
    print("Wrote \(url.path)")
}

/// Zoomed check: 32 and 16 px blown up 8x / 16x so pixels can be judged.
func makeZoom(_ names: [String], _ url: URL) {
    let vs = names.compactMap { n in variants.first { $0.name == n } }
    let cell: CGFloat = 256, pad: CGFloat = 20
    let width = pad + CGFloat(vs.count) * (cell + pad), height = pad + 2 * (cell + pad)
    let cs = CGColorSpace(name: CGColorSpace.sRGB)!
    let ctx = CGContext(data: nil, width: Int(width), height: Int(height), bitsPerComponent: 8, bytesPerRow: 0,
                        space: cs, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
    ctx.setFillColor(rgb(0x808080)); ctx.fill(CGRect(x: 0, y: 0, width: width, height: height))
    ctx.interpolationQuality = .none
    for (i, v) in vs.enumerated() {
        let x = pad + CGFloat(i) * (cell + pad)
        ctx.draw(render(v, size: 32), in: CGRect(x: x, y: pad + cell + pad, width: cell, height: cell))
        ctx.draw(render(v, size: 16), in: CGRect(x: x, y: pad, width: cell, height: cell))
    }
    writePNG(ctx.makeImage()!, url)
    print("Wrote \(url.path)")
}

// MARK: Variants

enum Layout { case row, stack }

struct Variant {
    let name: String, palette: Palette, layout: Layout
    var note = ""
    /// Exploration variants v01-v10 draw themselves; nil = the original row/stack drawing.
    var draw: ((R) -> Void)? = nil
}

let variants = [
    Variant(name: "dark-row", palette: dark, layout: .row),
    Variant(name: "light-row", palette: light, layout: .row),
    Variant(name: "dark-stack", palette: dark, layout: .stack),
] + explorations

// MARK: Shape

/// Apple's macOS icon grid: on a 1024 canvas the tile is 824 x 824, centred (100 px
/// margin), with a continuous-corner ("squircle") outline of ~185 px corner radius
/// and a soft drop shadow below it.
func tilePath(_ r: CGRect) -> CGPath {
    // Superellipse |x|^n + |y|^n = 1 confined to the corners: straight edges, with
    // continuous-curvature corners of radius `rad` (n = 5 approximates Apple's shape).
    let rad = r.width * 185.4 / 824
    let n = 5.0
    let path = CGMutablePath()
    let corners: [(CGFloat, CGFloat, Double)] = [   // corner centre, start angle
        (r.maxX - rad, r.maxY - rad, 0), (r.minX + rad, r.maxY - rad, .pi / 2),
        (r.minX + rad, r.minY + rad, .pi), (r.maxX - rad, r.minY + rad, 3 * .pi / 2),
    ]
    // Each corner is a quarter superellipse of size 1.28*rad, so the curve eases into the edge.
    let s = rad * 1.28
    var first = true
    for (cx, cy, a0) in corners {
        // Shift the centre so the quarter-curve of size s ends on the edges.
        let ox = cx + (cos(a0 + .pi / 4) > 0 ? -(s - rad) : (s - rad))
        let oy = cy + (sin(a0 + .pi / 4) > 0 ? -(s - rad) : (s - rad))
        for i in 0...48 {
            let t = a0 + Double(i) / 48 * .pi / 2
            let c = cos(t), sn = sin(t)
            let px = ox + s * CGFloat(copysign(pow(abs(c), 2 / n), c))
            let py = oy + s * CGFloat(copysign(pow(abs(sn), 2 / n), sn))
            if first { path.move(to: CGPoint(x: px, y: py)); first = false } else { path.addLine(to: CGPoint(x: px, y: py)) }
        }
    }
    path.closeSubpath()
    return path
}

// MARK: Drawing

func font(_ size: CGFloat, weight: NSFont.Weight) -> CTFont {
    NSFont.monospacedSystemFont(ofSize: size, weight: weight) as CTFont
}

/// Draws one glyph run with its baseline-left at `origin`.
func drawText(_ s: String, _ f: CTFont, _ color: CGColor, at origin: CGPoint, in ctx: CGContext) {
    let attr = NSAttributedString(string: s, attributes: [
        NSAttributedString.Key(kCTFontAttributeName as String): f,
        NSAttributedString.Key(kCTForegroundColorAttributeName as String): color,
    ])
    let line = CTLineCreateWithAttributedString(attr)
    ctx.textPosition = origin
    CTLineDraw(line, ctx)
}

func advance(_ f: CTFont) -> CGFloat {
    var g: CGGlyph = 0
    var ch: UniChar = 0x6D  // "m"
    CTFontGetGlyphsForCharacters(f, &ch, &g, 1)
    var adv = CGSize.zero
    CTFontGetAdvancesForGlyphs(f, .horizontal, &g, &adv, 1)
    return adv.width
}

/// Snaps a rect to whole device pixels (keeps small sizes crisp).
func snap(_ r: CGRect, _ px: CGFloat) -> CGRect {
    let x0 = (r.minX * px).rounded() / px, y0 = (r.minY * px).rounded() / px
    let x1 = max((r.maxX * px).rounded(), (r.minX * px).rounded() + 1) / px
    let y1 = max((r.maxY * px).rounded(), (r.minY * px).rounded() + 1) / px
    return CGRect(x: x0, y: y0, width: x1 - x0, height: y1 - y0)
}

/// Draws `v` into a `size` x `size` pixel context. All geometry is in 1024 units.
func render(_ v: Variant, size: Int) -> CGImage {
    let cs = CGColorSpace(name: CGColorSpace.sRGB)!
    let ctx = CGContext(data: nil, width: size, height: size, bitsPerComponent: 8, bytesPerRow: 0,
                        space: cs, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
    let scale = CGFloat(size) / 1024
    let px = 1 / scale  // one device pixel, in 1024 units
    ctx.scaleBy(x: scale, y: scale)
    ctx.setShouldSmoothFonts(false)
    if let draw = v.draw {
        draw(R(ctx: ctx, size: size))
        return ctx.makeImage()!
    }
    let p = v.palette
    let small = size <= 32

    // Tile with drop shadow (Apple template: y -12, blur 28, black 30%; omitted at tiny sizes).
    let tile = CGRect(x: 100, y: 100, width: 824, height: 824)
    let path = tilePath(tile)
    ctx.saveGState()
    if !small { ctx.setShadow(offset: CGSize(width: 0, height: -12 * scale), blur: 28 * scale, color: rgb(0, 0.3)) }
    ctx.addPath(path); ctx.setFillColor(p.bgBottom); ctx.fillPath()
    ctx.restoreGState()
    ctx.saveGState()
    ctx.addPath(path); ctx.clip()
    let grad = CGGradient(colorsSpace: cs, colors: [p.bgTop, p.bgBottom] as CFArray, locations: [0, 1])!
    ctx.drawLinearGradient(grad, start: CGPoint(x: 0, y: tile.maxY), end: CGPoint(x: 0, y: tile.minY), options: [])
    ctx.restoreGState()
    // Hairline edge so the tile holds its shape on a matching background.
    ctx.saveGState()
    ctx.addPath(path); ctx.clip()
    ctx.addPath(path); ctx.setStrokeColor(p.edge); ctx.setLineWidth(max(8, 2 * px)); ctx.strokePath()
    ctx.restoreGState()

    // Small sizes get heavier strokes; Core Text has no hinting, so weight carries legibility.
    let weight: NSFont.Weight = small ? .heavy : .bold
    switch v.layout {
    case .row:
        // "km" | "ux", optically centred on the tile.
        let f = font(small ? 290 : 240, weight: weight)
        let adv = advance(f)
        let gap = adv * 0.62  // the bar's slot
        let total = adv * 4 + gap
        var x = 512 - total / 2
        let xh = CTFontGetXHeight(f), asc = CTFontGetCapHeight(f)
        let base = 512 - xh / 2 - (asc - xh) * 0.2
        for (ch, c) in [("k", p.k), ("m", p.m)] { drawText(ch, f, c, at: CGPoint(x: x, y: base), in: ctx); x += adv }
        let barW = max(adv * 0.16, 1.4 * px)
        let bar = CGRect(x: x + gap / 2 - barW / 2, y: base - xh * 0.42, width: barW, height: asc + xh * 0.78)
        ctx.setFillColor(p.bar); ctx.fill(small ? snap(bar, scale) : bar)
        x += gap
        for (ch, c) in [("u", p.u), ("x", p.x)] { drawText(ch, f, c, at: CGPoint(x: x, y: base), in: ctx); x += adv }
    case .stack:
        // "km" over "ux", the bar as a horizontal split between the rows.
        let f = font(small ? 440 : 360, weight: weight)
        let adv = advance(f)
        let xh = CTFontGetXHeight(f), asc = CTFontGetAscent(f) * 0.74  // ~ascender of "k"
        let x0 = 512 - adv
        let rowGap = xh * 0.62
        // Centre the block from the bottom of "ux" to the top of "k"'s ascender.
        let shift = -(asc - xh) / 2
        func pix(_ v: CGFloat) -> CGFloat { small ? (v * scale).rounded() / scale : v }
        let topBase = pix(512 + rowGap / 2 + shift)
        let botBase = pix(512 - rowGap / 2 - xh + shift)
        drawText("k", f, p.k, at: CGPoint(x: x0, y: topBase), in: ctx)
        drawText("m", f, p.m, at: CGPoint(x: x0 + adv, y: topBase), in: ctx)
        drawText("u", f, p.u, at: CGPoint(x: x0, y: botBase), in: ctx)
        drawText("x", f, p.x, at: CGPoint(x: x0 + adv, y: botBase), in: ctx)
        _ = asc
        let barH = max(xh * 0.11, 1.4 * px)
        let bar = CGRect(x: x0 - adv * 0.05, y: (topBase + botBase + xh) / 2 - barH / 2, width: adv * 2.1, height: barH)
        ctx.setFillColor(p.bar); ctx.fill(small ? snap(bar, scale) : bar)
    }
    return ctx.makeImage()!
}

// MARK: Font files (v12 in fonts that aren't on the system)
//
// --font-file PATH renders v12 in a font file; --font-sheet / --font-shortlist render many from
// a list written by fetch-icon-fonts.py. Fonts are registered for this process only, never
// installed.

struct FontEntry { let label: String, path: String, note: String }

func readFontList(_ path: String) -> [FontEntry] {
    guard let text = try? String(contentsOfFile: path, encoding: .utf8) else { fatalError("can't read \(path)") }
    let dir = URL(fileURLWithPath: path).deletingLastPathComponent()
    return text.split(separator: "\n").compactMap { line in
        let f = line.split(separator: "\t", omittingEmptySubsequences: false).map(String.init)
        guard f.count >= 4 else { return nil }
        let p = f[1].isEmpty || f[1].hasPrefix("/") ? f[1] : dir.appendingPathComponent(f[1]).path
        return FontEntry(label: f[0], path: p, note: f[3])
    }
}

/// Registers the font for this process (CTFontManager, .process scope) and returns its descriptor.
func loadFontFile(_ path: String) -> CTFontDescriptor? {
    let url = URL(fileURLWithPath: path) as CFURL
    CTFontManagerRegisterFontsForURL(url, .process, nil)
    return (CTFontManagerCreateFontDescriptorsFromURL(url) as? [CTFontDescriptor])?.first
}

/// v12 drawn in a font file, or nil plus the reason when the font can't draw "kmux".
func v12FileVariant(_ e: FontEntry) -> (Variant?, String) {
    guard !e.path.isEmpty, let d = loadFontFile(e.path) else { return (nil, "FAIL: no font file") }
    let f = CTFontCreateWithFontDescriptor(d, 100, nil)
    let missing = ["k", "m", "u", "x"].filter { ch in
        var u = Array(ch.utf16), g: CGGlyph = 0
        return !CTFontGetGlyphsForCharacters(f, &u, &g, 1) || g == 0 || glyphPath(ch, f).boundingBoxOfPath.isNull
    }
    if !missing.isEmpty { return (nil, "FAIL: no glyph for \(missing.joined(separator: " "))") }
    let name = (CTFontCopyFullName(f) as String)
    let v = Variant(name: e.label, palette: light, layout: .stack, note: name,
                    draw: { r in v12Font(r, { CTFontCreateWithFontDescriptor(d, $0, nil) }, tracking: 0, measured: true) })
    return (v, name)
}

/// Grids of v12 in many fonts: each cell is the icon at 256 px, the 32 px icon below it, and
/// the font's name. Writes PREFIX-01.png, PREFIX-02.png, ... `perSheet` cells each.
func makeFontSheets(_ entries: [(String, Variant?, String)], prefix: String, perSheet: Int = 20) {
    let cols = 5, cellW: CGFloat = 300, cellH: CGFloat = 400
    for start in stride(from: 0, to: entries.count, by: perSheet) {
        let page = Array(entries[start..<min(start + perSheet, entries.count)])
        let rows = (page.count + cols - 1) / cols
        let width = CGFloat(cols) * cellW, height = CGFloat(rows) * cellH
        let ctx = CGContext(data: nil, width: Int(width), height: Int(height), bitsPerComponent: 8, bytesPerRow: 0,
                            space: CGColorSpace(name: CGColorSpace.sRGB)!, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
        ctx.setFillColor(rgb(0xececec)); ctx.fill(CGRect(x: 0, y: 0, width: width, height: height))
        ctx.interpolationQuality = .none
        for (i, (label, v, note)) in page.enumerated() {
            let x0 = CGFloat(i % cols) * cellW, y0 = height - CGFloat(i / cols + 1) * cellH
            if let v {
                ctx.draw(render(v, size: 256), in: CGRect(x: x0 + 22, y: y0 + 120, width: 256, height: 256))
                ctx.draw(render(v, size: 32), in: CGRect(x: x0 + 134, y: y0 + 78, width: 32, height: 32))
            } else {
                drawText("not rendered", font(22, weight: .bold), rgb(0xcf3f3f), at: CGPoint(x: x0 + 70, y: y0 + 240), in: ctx)
            }
            drawText(String("\(start + i + 1). \(label)".prefix(28)), font(17, weight: .semibold), rgb(0x1d2026), at: CGPoint(x: x0 + 14, y: y0 + 46), in: ctx)
            drawText(String(note.prefix(34)), font(13, weight: .regular), note.hasPrefix("FAIL") ? rgb(0xcf3f3f) : rgb(0x6b7280),
                     at: CGPoint(x: x0 + 14, y: y0 + 22), in: ctx)
        }
        let url = URL(fileURLWithPath: String(format: "%@-%02d.png", prefix, start / perSheet + 1))
        writePNG(ctx.makeImage()!, url)
        print("Wrote \(url.path)")
    }
}

// MARK: The icon

/// The kmux icon: v12 in the bundled Fira Mono Bold.
func iconVariant() -> Variant {
    let path = scriptDir.appendingPathComponent("fonts/FiraMono-Bold.ttf").path
    let (v, why) = v12FileVariant(FontEntry(label: "kmux", path: path, note: ""))
    guard let v else { fatalError("\(path): \(why)") }
    return v
}

/// Final check: the icon at 1024, 128, 64, 32 and 16 px on a light and a dark backdrop.
func makeFinalPreview(_ url: URL) {
    let v = iconVariant(), sizes = [1024, 128, 64, 32, 16], pad: CGFloat = 48
    let half = pad + sizes.reduce(0) { $0 + CGFloat($1) + pad }
    let height = 1024 + 2 * pad + 40
    let ctx = CGContext(data: nil, width: Int(2 * half), height: Int(height), bitsPerComponent: 8, bytesPerRow: 0,
                        space: CGColorSpace(name: CGColorSpace.sRGB)!, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
    ctx.interpolationQuality = .none
    for (j, bg) in [(rgb(0xececec), rgb(0x1d2026)), (rgb(0x2b2e33), rgb(0xe6e8eb))].enumerated() {
        let x0 = CGFloat(j) * half
        ctx.setFillColor(bg.0); ctx.fill(CGRect(x: x0, y: 0, width: half, height: height))
        var x = x0 + pad
        for s in sizes {
            ctx.draw(render(v, size: s), in: CGRect(x: x, y: pad + (1024 - CGFloat(s)) / 2, width: CGFloat(s), height: CGFloat(s)))
            drawText("\(s)", font(20, weight: .medium), bg.1, at: CGPoint(x: x + CGFloat(s) / 2 - 14, y: height - 34), in: ctx)
            x += CGFloat(s) + pad
        }
    }
    writePNG(ctx.makeImage()!, url)
    print("Wrote \(url.path)")
}

// MARK: Output

func writePNG(_ img: CGImage, _ url: URL) {
    let rep = NSBitmapImageRep(cgImage: img)
    try! rep.representation(using: .png, properties: [:])!.write(to: url)
}

func makeIcns(_ v: Variant, outDir: URL) {
    let iconset = outDir.appendingPathComponent("kmux.iconset")
    try? FileManager.default.removeItem(at: iconset)
    try! FileManager.default.createDirectory(at: iconset, withIntermediateDirectories: true)
    for pt in [16, 32, 128, 256, 512] {
        writePNG(render(v, size: pt), iconset.appendingPathComponent("icon_\(pt)x\(pt).png"))
        writePNG(render(v, size: pt * 2), iconset.appendingPathComponent("icon_\(pt)x\(pt)@2x.png"))
    }
    let icns = outDir.appendingPathComponent("kmux.icns")
    let p = Process()
    p.executableURL = URL(fileURLWithPath: "/usr/bin/iconutil")
    p.arguments = ["-c", "icns", iconset.path, "-o", icns.path]
    try! p.run(); p.waitUntilExit()
    guard p.terminationStatus == 0 else { fatalError("iconutil failed") }
    try? FileManager.default.removeItem(at: iconset)
    print("Wrote \(icns.path) (\(v.name))")
}

/// One row per variant: 1024, 128, 32, 16 px at actual size, plus 32 and 16 blown up 8x and 16x
/// (nearest neighbour) so their pixels can be judged.
func makePreview(_ url: URL) {
    let sizes = [1024, 128, 32, 16]
    let pad: CGFloat = 40, rowH: CGFloat = 1024 + 2 * pad, zoom: CGFloat = 8
    let width = pad + 1024 + pad + 128 + pad + 32 + pad + 16 + pad + 32 * zoom + pad + 16 * zoom * 2 + pad
    let height = rowH * CGFloat(variants.count)
    let cs = CGColorSpace(name: CGColorSpace.sRGB)!
    let ctx = CGContext(data: nil, width: Int(width), height: Int(height), bitsPerComponent: 8, bytesPerRow: 0,
                        space: cs, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
    for (i, v) in variants.enumerated() {
        let y0 = height - rowH * CGFloat(i + 1)
        // Alternate backdrop so both light and dark Docks are represented.
        ctx.setFillColor(i % 2 == 0 ? rgb(0xd0d4da) : rgb(0x3a3f47))
        ctx.fill(CGRect(x: 0, y: y0, width: width, height: rowH))
        var x = pad
        for s in sizes {
            let img = render(v, size: s)
            ctx.interpolationQuality = .none
            ctx.draw(img, in: CGRect(x: x, y: y0 + pad, width: CGFloat(s), height: CGFloat(s)))
            x += CGFloat(s) + pad
        }
        for s in [32, 16] {
            let z = zoom * CGFloat(32 / s)
            ctx.interpolationQuality = .none
            ctx.draw(render(v, size: s), in: CGRect(x: x, y: y0 + pad, width: CGFloat(s) * z, height: CGFloat(s) * z))
            x += CGFloat(s) * z + pad
        }
        drawText(v.name, font(28, weight: .medium), i % 2 == 0 ? rgb(0x1d2026) : rgb(0xe6e8eb),
                 at: CGPoint(x: 1024 + 2 * pad, y: y0 + rowH - pad - 28), in: ctx)
    }
    writePNG(ctx.makeImage()!, url)
    print("Wrote \(url.path)")
}

var args = Array(CommandLine.arguments.dropFirst())
func option(_ name: String) -> String? {
    guard let i = args.firstIndex(of: name), i + 1 < args.count else { return nil }
    return args[i + 1]
}
let scriptDir = URL(fileURLWithPath: CommandLine.arguments[0]).deletingLastPathComponent()
if let q = option("--quad") { quadMax = Int(q) ?? quadMax }   // largest size drawn as quadrants
if let preview = option("--preview") {
    makePreview(URL(fileURLWithPath: preview))
} else if let sheet = option("--sheet") {
    // --sheet out.png [--only v01,v02,...]: contact sheet of the exploration variants.
    let names = option("--only")?.split(separator: ",").map(String.init) ?? explorations.map(\.name)
    makeSheet(names, URL(fileURLWithPath: sheet))
} else if let sheet = option("--sheet2") {
    // --sheet2 out.png [--only v11,...]: round-two sheet, 32 px shown both lettered and as quadrants.
    makeSheet2(option("--only")?.split(separator: ",").map(String.init) ?? ["v11", "v12", "v13", "v14", "v15", "v16"],
               URL(fileURLWithPath: sheet))
} else if let fam = option("--family") {
    // --family out.png --kanna kanna-icon.png [--only v11,v14]: side by side with Kanna.app's icon.
    makeFamily(URL(fileURLWithPath: option("--kanna") ?? "kanna-icon.png"),
               option("--only")?.split(separator: ",").map(String.init) ?? ["v11", "v12", "v14"], URL(fileURLWithPath: fam))
} else if let prefix = option("--font-sheet") {
    // --font-sheet PREFIX --font-list fonts.tsv: v12 in every listed font, ~20 per sheet,
    // after the built-in v12 and v12b for comparison.
    var entries: [(String, Variant?, String)] = ["v12", "v12b"].compactMap { n in
        variants.first { $0.name == n }.map { (n == "v12" ? "v12 (SF Rounded Heavy)" : "v12b (SF Pro Heavy)", $0, "built in") }
    }
    for e in readFontList(option("--font-list") ?? "fonts.tsv") {
        let (v, why) = v12FileVariant(e)
        entries.append((e.label, v, v == nil ? why : "\(why) | \(e.note)"))
        if v == nil { print("\(e.label): \(why)") }
    }
    makeFontSheets(entries, prefix: prefix)
} else if let out = option("--font-shortlist") {
    // --font-shortlist out.png --font-list fonts.tsv --only "Hack,VT323": 512/64/32 on light and dark.
    let want = (option("--only") ?? "").split(separator: ",").map(String.init)
    let list = readFontList(option("--font-list") ?? "fonts.tsv")
    let vs = want.compactMap { w in list.first { $0.label == w }.flatMap { v12FileVariant($0).0 } }
    makeSheet2(vs, URL(fileURLWithPath: out), columns: option("--sizes").map(parseColumns) ?? defaultColumns)
} else if let file = option("--font-file") {
    // --font-file PATH [--out DIR]: kmux.icns of v12 in that font (registered for this process only).
    let (v, why) = v12FileVariant(FontEntry(label: "v12-file", path: file, note: ""))
    guard let v else { fatalError(why) }
    makeIcns(v, outDir: option("--out").map { URL(fileURLWithPath: $0) } ?? scriptDir)
} else if let out = option("--final-preview") {
    makeFinalPreview(URL(fileURLWithPath: out))
} else if let zoom = option("--zoom") {
    let names = option("--only")?.split(separator: ",").map(String.init) ?? explorations.map(\.name)
    makeZoom(names, URL(fileURLWithPath: zoom))
} else {
    let name = option("--variant") ?? "kmux"
    guard let v = name == "kmux" ? iconVariant() : variants.first(where: { $0.name == name }) else {
        fatalError("unknown variant \(name); one of \(["kmux"] + variants.map(\.name))")
    }
    makeIcns(v, outDir: option("--out").map { URL(fileURLWithPath: $0) } ?? scriptDir)
}
