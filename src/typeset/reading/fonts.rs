//! The fonts reading mode lays text out in: those of the fontdb database
//! that SVG images use, handed to parley, with the fallbacks kiri lists
//! for emoji and for the scripts the sans-serif font may lack.

use std::cell::RefCell;
use std::collections::HashSet;
use std::fmt;

use parley::fontique::{Blob, Collection, CollectionOptions, GenericFamily, Script, SourceCache};
use parley::{FontContext, LayoutContext};
use resvg::usvg::fontdb;

use crate::images::{self, EMOJI, SCRIPTS, installed};
use crate::typeset::reading::text::Brush;

/// The fonts, loaded when text is first laid out in them, and parley's
/// contexts, which are kept for the next layout.
pub struct Fonts {
    source: images::Fonts,
    contexts: RefCell<Option<Contexts>>,
}

/// What parley lays text out with: the fonts, and room it reuses.
pub(crate) struct Contexts {
    pub fonts: FontContext,
    pub layout: LayoutContext<Brush>,
}

impl Fonts {
    /// The fonts installed on the system.
    pub fn system() -> Self {
        Self::new(images::Fonts::system())
    }

    /// The fixture fonts, so that tests do not depend on what is
    /// installed: DejaVu Sans and Noto Sans Mono.
    #[cfg(test)]
    pub(crate) fn fixture() -> Self {
        Self::new(images::Fonts::reading_fixture())
    }

    fn new(source: images::Fonts) -> Self {
        Self {
            source,
            contexts: RefCell::new(None),
        }
    }

    /// What `lay_out` returns, given parley's contexts.
    pub(crate) fn with<T>(&self, lay_out: impl FnOnce(&mut Contexts) -> T) -> T {
        let mut contexts = self.contexts.borrow_mut();
        let contexts = contexts.get_or_insert_with(|| Contexts {
            fonts: FontContext {
                collection: collection(&self.source.database()),
                source_cache: SourceCache::default(),
            },
            layout: LayoutContext::new(),
        });
        lay_out(contexts)
    }
}

impl fmt::Debug for Fonts {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Fonts")
            .field("loaded", &self.contexts.borrow().is_some())
            .finish()
    }
}

/// The fonts of `database` as parley's collection, with the generic
/// families the database names and the fallbacks kiri lists.
fn collection(database: &fontdb::Database) -> Collection {
    let mut collection = Collection::new(CollectionOptions {
        shared: false,
        system_fonts: false,
    });
    // Each file is mapped once and shared by all its faces; the database
    // itself is shared with SVG images and stays as it is.
    let mut database = database.clone();
    let mut files = HashSet::new();
    let faces: Vec<_> = (database.faces())
        .map(|face| (face.id, face.source.clone()))
        .collect();
    for (id, source) in faces {
        let data = match source {
            fontdb::Source::File(path) | fontdb::Source::SharedFile(path, _) => {
                if !files.insert(path) {
                    continue;
                }
                // SAFETY: the file is mapped as resvg maps it for SVG text,
                // which also takes font files not to change while kiri runs.
                unsafe { database.make_shared_face_data(id) }.map(|(data, _)| data)
            }
            fontdb::Source::Binary(data) => Some(data),
        };
        if let Some(data) = data {
            collection.register_fonts(Blob::new(data), None);
        }
    }
    let names: HashSet<&str> = (database.faces())
        .flat_map(|face| face.families.iter().map(|(name, _)| name.as_str()))
        .collect();
    let mut families = |names: &[&str]| -> Vec<_> {
        (names.iter())
            .filter_map(|name| collection.family_id(name))
            .collect()
    };
    let sans_serif = families(&[database.family_name(&fontdb::Family::SansSerif)]);
    let monospace = families(&[database.family_name(&fontdb::Family::Monospace)]);
    let emoji = families(&installed(EMOJI, &names));
    let scripts: Vec<_> = (SCRIPTS.iter())
        .map(|(script, fonts)| (*script, families(&installed(fonts, &names))))
        .collect();
    collection.set_generic_families(GenericFamily::SansSerif, sans_serif.into_iter());
    collection.set_generic_families(GenericFamily::Monospace, monospace.into_iter());
    collection.set_generic_families(GenericFamily::Emoji, emoji.into_iter());
    for (script, fonts) in scripts {
        let mut code = [0; 4];
        code.copy_from_slice(&script.as_bytes()[..4]);
        collection.set_fallbacks(Script::from_bytes(code), fonts.into_iter());
    }
    collection
}
