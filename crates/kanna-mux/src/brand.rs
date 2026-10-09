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
    /// A program every terminal runs through, if the product hosts its own
    /// terminals (Kanna keeps them alive across app restarts): kmux runs
    /// `WRAPPER` for a shell, or `WRAPPER -- SHELL -c COMMAND` for a command.
    pub terminal_wrapper: Option<String>,
}

static CURRENT: OnceLock<Brand> = OnceLock::new();

impl Brand {
    pub fn new(id: &str, display_name: &str) -> Brand {
        Brand { id: id.into(), display_name: display_name.into(), terminal_wrapper: None }
    }

    pub fn kmux() -> Brand {
        Brand::new("kmux", "kmux")
    }

    /// This process's brand: the one the app set, else kmux.
    /// `KMUX_TERMINAL_WRAPPER` (an absolute path) overrides the wrapper, for tests.
    pub fn current() -> &'static Brand {
        CURRENT.get_or_init(|| {
            let mut brand = Brand::kmux();
            brand.terminal_wrapper = std::env::var("KMUX_TERMINAL_WRAPPER").ok().filter(|w| !w.is_empty());
            brand
        })
    }

    /// Sets the brand, once, before anything reads it. The app resolves a
    /// wrapper inside its bundle to an absolute path first.
    pub fn set_current(brand: Brand) -> Result<(), Brand> {
        CURRENT.set(brand)
    }

    /// `KMUX_SOCKET` for kmux, `KANNA_SOCKET` for Kanna.
    pub fn variable(&self, name: &str) -> String {
        format!("{}_{name}", self.id.to_uppercase())
    }
}
