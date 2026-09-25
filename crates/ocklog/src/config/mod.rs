pub mod theme;
pub use theme::UiTheme;

pub fn clear_registry() {
    // Kept to mirror safety teardown for thread-local registries
}
