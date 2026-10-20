//! Front matter: the metadata block at the start of a document.

use kiri::caps::Capabilities;
use kiri::render::Inputs;

mod common;

use common::{WIDTH, render};

#[test]
fn front_matter_shows_as_a_code_block_in_its_language() {
    for caps in [Capabilities::plain(WIDTH), Capabilities::kitty(WIDTH)] {
        let inputs = Inputs::new(caps);
        for (front_matter, code_block) in [
            ("---\ntitle: A\n---\n", "```yaml\ntitle: A\n```\n"),
            ("+++\ntitle = 'A'\n+++\n", "```toml\ntitle = 'A'\n```\n"),
            ("---php\n$a = 1;\n---\n", "```php\n$a = 1;\n```\n"),
        ] {
            assert_eq!(
                render(&format!("{front_matter}# Body\n"), &inputs),
                render(&format!("{code_block}\n# Body\n"), &inputs),
                "{front_matter}"
            );
        }
    }
}
