use std::sync::OnceLock;

/// Which product this is: kmux itself, or a product built on it (Kanna
/// ships its own copy as Kanna.app). The brand names the socket folder and
/// files and the environment variables, so the two never talk to each other's
/// windows. The app sets it once at launch (`Brand::set_current`); plain
/// builds and tests are kmux.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Brand {
    /// Lower case: the socket folder and files (`kmux`, `kmux.sock`).
    pub id: String,
    /// What the user sees: window title, app menu.
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

    /// This process's brand: the one the app set, else kmux.
    pub fn current() -> &'static Brand {
        CURRENT.get_or_init(Brand::kmux)
    }

    /// Sets the brand, once, before anything reads it.
    pub fn set_current(brand: Brand) -> Result<(), Brand> {
        CURRENT.set(brand)
    }

    /// `KMUX_SOCKET` for kmux, `KANNA_SOCKET` for Kanna.
    pub fn variable(&self, name: &str) -> String {
        format!("{}_{name}", self.id.to_uppercase())
    }
}
