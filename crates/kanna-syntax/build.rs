fn main() {
    let vue = std::path::Path::new("vendor/tree-sitter-vue/src");
    cc::Build::new()
        .include(vue)
        .file(vue.join("parser.c"))
        .file(vue.join("scanner.c"))
        .flag_if_supported("-Wno-unused-parameter")
        .flag_if_supported("-Wno-unused-but-set-variable")
        .compile("tree-sitter-vue");
    println!("cargo:rerun-if-changed=vendor/tree-sitter-vue/src");
}
