import AppKit

/// Git graphs: merman places branches, commits and the arrows between them
/// (as SVG path data); branch and commit labels and tags are placed here as
/// Mermaid's gitGraph renderer does, since merman leaves them to its SVG.
enum GitGraph {
    static let commitLabelSize: CGFloat = 10
    static let branchLabelSize: CGFloat = 16

    static func scene(semantic: [String: Any], layout: [String: Any]) -> DiagramScene {
        let frame = Frame(layout: layout)
        var scene = DiagramScene(size: frame.size)
        let direction = layout.string("direction") ?? "LR"
        let vertical = direction == "TB" || direction == "BT"
        let maxPos = layout.double("max_pos") ?? 0
        let rotate = layout["rotate_commit_label"] as? Bool ?? true
        let branches = layout.list("branches")
        let branchIndex = Dictionary(branches.compactMap { b in b.string("name").map { ($0, Int(b.double("index") ?? 0)) } }, uniquingKeysWith: { a, _ in a })
        var move = CGAffineTransform(translationX: frame.offset.x, y: frame.offset.y)
        func shape(_ path: CGPath, fill: Paint? = nil, stroke: Paint? = nil, lineWidth: CGFloat = 1, dash: [CGFloat] = []) {
            scene.shape(path.copy(using: &move) ?? path, fill: fill, stroke: stroke, lineWidth: lineWidth, dash: dash)
        }
        func text(_ string: String, in rect: CGRect, size: CGFloat, color: Paint = .role(.text), alignment: NSTextAlignment = .center, angle: CGFloat = 0) {
            scene.text(string, in: frame.rect(rect), size: size, color: color, htmlLike: false, alignment: alignment, fixedSize: true, angle: angle)
        }

        if layout["show_branches"] as? Bool ?? true {
            for branch in branches {
                let index = Int(branch.double("index") ?? 0), pos = branch.double("pos") ?? 0
                let name = branch.string("name") ?? ""
                let width = max(branch.double("bbox_width") ?? 0, LabelText.width(name, font: LabelText.font(size: branchLabelSize)))
                let height = branch.double("bbox_height") ?? branchLabelSize * 1.1
                let line: [CGPoint], box: CGRect, textX: CGFloat
                switch direction {
                case "TB", "BT":
                    line = [CGPoint(x: pos, y: 30), CGPoint(x: pos, y: maxPos)]
                    box = CGRect(x: pos - width / 2 - 10, y: direction == "TB" ? 0 : maxPos, width: width + 18, height: height + 4)
                    textX = pos - width / 2 - 5
                default:
                    let spine = pos - 2
                    let pad: CGFloat = rotate ? 30 : 0
                    line = [CGPoint(x: 0, y: spine), CGPoint(x: maxPos, y: spine)]
                    box = CGRect(x: -width - 4 - pad - 19, y: spine - 12 - height / 2 + 10, width: width + 18, height: height + 4)
                    textX = -width - 14 - pad
                }
                shape(Paths.line(line), stroke: .role(.branchLine), dash: [2, 2])
                shape(Paths.rect(box, radius: 4), fill: .role(.branch(index)))
                text(name, in: CGRect(x: textX, y: box.minY, width: width, height: box.height), size: branchLabelSize, color: .role(.branchText(index)))
            }
        }

        for arrow in layout.list("arrows") {
            guard let d = arrow.string("d") else { continue }
            shape(path(d), stroke: .role(.branch(Int(arrow.double("class_index") ?? 0))), lineWidth: 8)
        }

        let commits = layout.list("commits")
        for commit in commits {
            guard let x = commit.double("x"), let y = commit.double("y") else { continue }
            let index = commit.string("branch").flatMap { branchIndex[$0] } ?? 0
            let type = Int(commit.double("custom_type") ?? commit.double("commit_type") ?? 0)
            func circle(_ cx: CGFloat, _ cy: CGFloat, _ r: CGFloat) -> CGPath {
                CGPath(ellipseIn: CGRect(x: cx - r, y: cy - r, width: 2 * r, height: 2 * r), transform: nil)
            }
            switch type {
            case 2: // HIGHLIGHT
                shape(Paths.rect(CGRect(x: x - 10, y: y - 10, width: 20, height: 20)), fill: .role(.branchHighlight(index)))
                shape(Paths.rect(CGRect(x: x - 6, y: y - 6, width: 12, height: 12)), fill: .role(.commitInner))
            case 4: // cherry-pick: a cherry pair on a dark circle
                shape(circle(x, y, 10), fill: .role(.startFill))
                shape(circle(x - 3, y + 2, 2.75), fill: .role(.numberText))
                shape(circle(x + 3, y + 2, 2.75), fill: .role(.numberText))
                shape(Paths.line([CGPoint(x: x + 3, y: y + 1), CGPoint(x: x, y: y - 5), CGPoint(x: x - 3, y: y + 1)]), stroke: .role(.numberText))
            default:
                shape(circle(x, y, 10), fill: .role(.branch(index)))
                if type == 3 { shape(circle(x, y, 6), fill: .role(.commitInner)) } // merge
                if type == 1 { // REVERSE
                    let cross = CGMutablePath()
                    cross.addLines(between: [CGPoint(x: x - 5, y: y - 5), CGPoint(x: x + 5, y: y + 5)])
                    cross.addLines(between: [CGPoint(x: x - 5, y: y + 5), CGPoint(x: x + 5, y: y - 5)])
                    shape(cross, stroke: .role(.commitInner), lineWidth: 3)
                }
            }
        }

        // Labels go over every commit, so a rotated id never hides under the next one.
        let labelFont = LabelText.font(size: commitLabelSize)
        let labelHeight = (commitLabelSize * 1.1).rounded()
        for commit in commits {
            guard let id = commit.string("id"), let x = commit.double("x"), let y = commit.double("y") else { continue }
            let pos = commit.double("pos") ?? 0, offsetPos = commit.double("pos_with_offset") ?? x
            let type = Int(commit.double("commit_type") ?? 0)
            // As in Mermaid: merges show their id only if it was given, cherry-picks never.
            if (type != 3 || commit["custom_id"] as? Bool == true) && type != 4 && layout["show_commit_label"] as? Bool ?? true {
                let width = LabelText.width(id, font: labelFont)
                var box = CGRect(x: offsetPos - width / 2 - 2, y: y + 13.5, width: width + 4, height: labelHeight + 4)
                var turn = CGAffineTransform.identity
                if vertical { box = CGRect(x: x - (width + 21), y: y - 12, width: width + 4, height: labelHeight + 4) }
                if rotate {
                    turn = CGAffineTransform(translationX: x, y: y).rotated(by: -.pi / 4).translatedBy(x: -x, y: -y)
                    if !vertical {
                        // Mermaid turns an LR label about the commit's unshifted position, then moves it by its width.
                        turn = CGAffineTransform(translationX: -7.5 - (width + 10) / 25 * 9.5, y: 10 + width / 25 * 8.5)
                            .translatedBy(x: pos, y: y).rotated(by: -.pi / 4).translatedBy(x: -pos, y: -y)
                    }
                }
                var whole = turn
                shape(Paths.rect(box).copy(using: &whole) ?? Paths.rect(box), fill: .role(.commitLabelFill))
                let centre = CGPoint(x: box.midX, y: box.midY).applying(turn)
                text(id, in: CGRect(x: centre.x - box.width / 2, y: centre.y - box.height / 2, width: box.width, height: box.height),
                     size: commitLabelSize, angle: rotate ? -.pi / 4 : 0)
            }

            let tags = ((commit["tags"] as? [String]) ?? []).reversed()
            guard !tags.isEmpty else { continue }
            let tagWidth = tags.map { LabelText.width($0, font: labelFont) }.max() ?? 0
            let h2 = labelHeight / 2
            for (i, tag) in tags.enumerated() {
                let yOff = CGFloat(i) * 20
                if vertical {
                    let origin = pos + yOff
                    var turn = CGAffineTransform(translationX: 12, y: 12).translatedBy(x: x, y: pos).rotated(by: .pi / 4).translatedBy(x: -x, y: -pos)
                    let outline = Paths.polygon([CGPoint(x: x, y: origin + 2), CGPoint(x: x, y: origin - 2), CGPoint(x: x + 10, y: origin - h2 - 2),
                                                 CGPoint(x: x + 14 + tagWidth, y: origin - h2 - 2), CGPoint(x: x + 14 + tagWidth, y: origin + h2 + 2),
                                                 CGPoint(x: x + 10, y: origin + h2 + 2)])
                    shape(outline.copy(using: &turn) ?? outline, fill: .role(.tagFill), stroke: .role(.tagStroke))
                    shape(CGPath(ellipseIn: CGRect(x: x + 0.5, y: origin - 1.5, width: 3, height: 3), transform: &turn), fill: .role(.text))
                    let textTurn = CGAffineTransform(translationX: 14, y: 14).translatedBy(x: x, y: pos).rotated(by: .pi / 4).translatedBy(x: -x, y: -pos)
                    let centre = CGPoint(x: x + 5 + tagWidth / 2, y: origin).applying(textTurn)
                    text(tag, in: CGRect(x: centre.x - tagWidth / 2 - 1, y: centre.y - h2, width: tagWidth + 2, height: labelHeight),
                         size: commitLabelSize, angle: .pi / 4)
                } else {
                    let ly = y - 19.2 - yOff
                    shape(Paths.polygon([CGPoint(x: pos - tagWidth / 2 - 2, y: ly + 2), CGPoint(x: pos - tagWidth / 2 - 2, y: ly - 2),
                                         CGPoint(x: offsetPos - tagWidth / 2 - 4, y: ly - h2 - 2), CGPoint(x: offsetPos + tagWidth / 2 + 4, y: ly - h2 - 2),
                                         CGPoint(x: offsetPos + tagWidth / 2 + 4, y: ly + h2 + 2), CGPoint(x: offsetPos - tagWidth / 2 - 4, y: ly + h2 + 2)]),
                          fill: .role(.tagFill), stroke: .role(.tagStroke))
                    shape(CGPath(ellipseIn: CGRect(x: pos - tagWidth / 2 + 0.5, y: ly - 1.5, width: 3, height: 3), transform: nil), fill: .role(.text))
                    text(tag, in: CGRect(x: offsetPos - tagWidth / 2 - 1, y: ly - h2, width: tagWidth + 2, height: labelHeight), size: commitLabelSize)
                }
            }
        }
        return scene
    }

    /// An arrow's SVG path data (M, L and the quarter-circle A corners merman
    /// writes between them) as a path.
    static func path(_ d: String) -> CGPath {
        let path = CGMutablePath()
        let tokens = d.matches(of: /[MLA]|-?[0-9]*\.?[0-9]+(?:[eE][-+]?[0-9]+)?/).map { String($0.output) }
        var i = 0
        func number() -> CGFloat? {
            guard i < tokens.count, let value = Double(tokens[i]) else { return nil }
            i += 1
            return CGFloat(value)
        }
        while i < tokens.count {
            let command = tokens[i]
            i += 1
            switch command {
            case "M", "L":
                guard let x = number(), let y = number() else { return path }
                if command == "M" || path.isEmpty { path.move(to: CGPoint(x: x, y: y)) } else { path.addLine(to: CGPoint(x: x, y: y)) }
            case "A":
                guard let r = number(), number() != nil, number() != nil, number() != nil, let sweep = number(),
                      let x = number(), let y = number() else { return path }
                let a = path.currentPoint, b = CGPoint(x: x, y: y)
                // The centre is the corner whose turn matches the sweep (clockwise on screen when 1); the other corner is where the lines meet.
                let corners = [CGPoint(x: a.x, y: b.y), CGPoint(x: b.x, y: a.y)]
                let centre = corners.first { c in ((a.x - c.x) * (b.y - c.y) - (a.y - c.y) * (b.x - c.x) > 0) == (sweep != 0) } ?? corners[1]
                let meet = centre == corners[0] ? corners[1] : corners[0]
                path.addArc(tangent1End: meet, tangent2End: b, radius: r)
            default:
                return path
            }
        }
        return path
    }
}
