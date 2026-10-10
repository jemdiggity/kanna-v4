import Testing
@testable import KannaMarkdown

struct FrontMatterTests {
    @Test func frontMatterIsAYamlBlockOnTheSameLines() {
        let text = "---\nname: kanna-markup\ndescription: Use when\n---\n\n# Kanna markup\n"
        #expect(MarkdownRenderer.fencingFrontMatter(text) == "```yaml\nname: kanna-markup\ndescription: Use when\n```\n\n# Kanna markup\n")
    }

    @Test func aFenceInsideIsOutrun() {
        #expect(MarkdownRenderer.fencingFrontMatter("---\nx: ```\n...\nbody") == "````yaml\nx: ```\n````\nbody")
    }

    @Test func onlyALeadingBlockCounts() {
        for text in ["# Title\n---\na: b\n---\n", "---\nnever closed\n", "---\n"] {
            #expect(MarkdownRenderer.fencingFrontMatter(text) == text)
        }
    }
}
