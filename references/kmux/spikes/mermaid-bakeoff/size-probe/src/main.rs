// Lays out the diagram on stdin and prints merman's layout JSON (nothing without a feature).
use std::io::Read;

fn main() {
    let mut source = String::new();
    std::io::stdin().read_to_string(&mut source).unwrap();
    #[cfg(any(feature = "slim", feature = "full"))]
    {
        use merman::{OperationControl, RenderOutput, RenderRequest, Renderer, SvgRequest};
        let output = Renderer::new()
            .render(RenderRequest::layout_json(&source, OperationControl::new(), SvgRequest::default()))
            .expect("layout");
        if let RenderOutput::LayoutJson(Some(layout)) = output {
            println!("{}", serde_json::to_string(layout.layout()).unwrap());
        }
    }
    #[cfg(not(any(feature = "slim", feature = "full")))]
    println!("{}", serde_json::to_string(&source.len()).unwrap());
}
