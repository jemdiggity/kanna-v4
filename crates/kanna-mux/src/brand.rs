use std::sync::OnceLock;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Brand {
    pub id: String,
    pub display_name: String,
}

static CURRENT: OnceLock<Brand> = OnceLock::new();

impl Brand {
    pub fn new(id: &str, display_name: &str) -> Brand {
        Brand { id: id.into(), display_name: display_name.into() }
    }

    pub fn kmux() -> Brand {
        Brand::new("kmux", "kmux")
    }

    pub fn current() -> &'static Brand {
        CURRENT.get_or_init(Brand::kmux)
    }

    pub fn set_current(brand: Brand) -> Result<(), Brand> {
        CURRENT.set(brand)
    }

    pub fn variable(&self, name: &str) -> String {
        format!("{}_{name}", self.id.to_uppercase())
    }
}
