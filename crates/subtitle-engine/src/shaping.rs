use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FontCatalog {
    pub schema_version: u32,
    pub roles: Vec<FontRole>,
}

impl FontCatalog {
    #[must_use]
    pub fn role(&self, id: &str) -> Option<&FontRole> {
        self.roles.iter().find(|role| role.id == id)
    }

    pub fn validate(&self) -> Result<(), FontCatalogError> {
        if self.schema_version != 1 {
            return Err(FontCatalogError::UnsupportedSchema(self.schema_version));
        }
        if self.roles.is_empty() {
            return Err(FontCatalogError::NoRoles);
        }
        for (index, role) in self.roles.iter().enumerate() {
            if role.id.trim().is_empty() || role.system_families.is_empty() {
                return Err(FontCatalogError::InvalidRole(role.id.clone()));
            }
            if self.roles[..index]
                .iter()
                .any(|previous| previous.id == role.id)
            {
                return Err(FontCatalogError::DuplicateRole(role.id.clone()));
            }
            for font in &role.system_families {
                if font.family.trim().is_empty()
                    || font.platforms.is_empty()
                    || font.scripts.is_empty()
                    || font.license_id.trim().is_empty()
                    || font.source == FontSource::OperatingSystem && font.redistribute
                {
                    return Err(FontCatalogError::InvalidFont(font.family.clone()));
                }
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum FontCatalogError {
    #[error("font catalog schema {0} is not supported")]
    UnsupportedSchema(u32),
    #[error("font catalog contains no roles")]
    NoRoles,
    #[error("font role `{0}` is invalid")]
    InvalidRole(String),
    #[error("font role `{0}` is duplicated")]
    DuplicateRole(String),
    #[error("font family `{0}` has invalid source, license, or coverage metadata")]
    InvalidFont(String),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FontRole {
    pub id: String,
    pub system_families: Vec<SystemFontFamily>,
    pub generic_fallback: GenericFontFamily,
    pub shaping_features: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SystemFontFamily {
    pub family: String,
    pub platforms: Vec<String>,
    pub scripts: Vec<ScriptClass>,
    pub source: FontSource,
    pub license_id: String,
    pub redistribute: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FontSource {
    OperatingSystem,
    Bundled,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GenericFontFamily {
    SansSerif,
    Serif,
    Monospace,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScriptClass {
    Latin,
    Cyrillic,
    Greek,
    Arabic,
    Hebrew,
    Devanagari,
    Bengali,
    Thai,
    Han,
    HiraganaKatakana,
    Hangul,
    Emoji,
    Common,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextDirection {
    LeftToRight,
    RightToLeft,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ShapingPlan {
    pub locale: String,
    pub direction: TextDirection,
    pub scripts: Vec<ScriptClass>,
    pub font_role: String,
    pub family_fallback_chain: Vec<String>,
    pub generic_fallback: GenericFontFamily,
    /// OpenType features forwarded to a platform shaper such as DirectWrite or HarfBuzz.
    pub open_type_features: Vec<String>,
    pub require_bidi_reordering: bool,
    pub require_complex_shaping: bool,
    pub preserve_grapheme_clusters: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ShapingError {
    #[error("font role `{0}` does not exist")]
    UnknownFontRole(String),
}

pub fn plan_shaping(
    text: &str,
    locale: Option<&str>,
    font_role: &str,
    catalog: &FontCatalog,
) -> Result<ShapingPlan, ShapingError> {
    let role = catalog
        .role(font_role)
        .ok_or_else(|| ShapingError::UnknownFontRole(font_role.to_owned()))?;
    let scripts = scripts_in(text);
    let direction = paragraph_direction(text);
    let requested_scripts = scripts
        .iter()
        .copied()
        .filter(|script| !matches!(script, ScriptClass::Common | ScriptClass::Unknown))
        .collect::<Vec<_>>();
    let mut family_fallback_chain = role
        .system_families
        .iter()
        .filter(|font| {
            (requested_scripts.is_empty()
                && font
                    .scripts
                    .iter()
                    .any(|script| matches!(script, ScriptClass::Common | ScriptClass::Unknown)))
                || requested_scripts
                    .iter()
                    .any(|script| font.scripts.contains(script))
        })
        .map(|font| font.family.clone())
        .collect::<Vec<_>>();
    // Logical generic families are last-resort resolver hints. Keep them at the end even if their
    // actual script coverage is knowable only after the operating system resolves the family.
    let generic_families = role
        .system_families
        .iter()
        .filter(|font| font.scripts.contains(&ScriptClass::Unknown))
        .map(|font| font.family.clone())
        .collect::<Vec<_>>();
    for family in generic_families {
        if !family_fallback_chain.contains(&family) {
            family_fallback_chain.push(family);
        }
    }
    let require_complex_shaping = scripts.iter().any(|script| {
        matches!(
            script,
            ScriptClass::Arabic
                | ScriptClass::Hebrew
                | ScriptClass::Devanagari
                | ScriptClass::Bengali
                | ScriptClass::Thai
        )
    });

    Ok(ShapingPlan {
        locale: locale.unwrap_or("und").to_owned(),
        direction,
        scripts,
        font_role: font_role.to_owned(),
        family_fallback_chain,
        generic_fallback: role.generic_fallback,
        open_type_features: role.shaping_features.clone(),
        require_bidi_reordering: direction == TextDirection::RightToLeft,
        require_complex_shaping,
        preserve_grapheme_clusters: true,
    })
}

fn scripts_in(text: &str) -> Vec<ScriptClass> {
    let mut scripts = Vec::new();
    for character in text.chars() {
        let script = classify_script(character);
        if !scripts.contains(&script) {
            scripts.push(script);
        }
    }
    if scripts.is_empty() {
        scripts.push(ScriptClass::Common);
    }
    scripts
}

fn paragraph_direction(text: &str) -> TextDirection {
    text.chars()
        .find_map(|character| match classify_script(character) {
            ScriptClass::Arabic | ScriptClass::Hebrew => Some(TextDirection::RightToLeft),
            ScriptClass::Latin
            | ScriptClass::Cyrillic
            | ScriptClass::Greek
            | ScriptClass::Devanagari
            | ScriptClass::Bengali
            | ScriptClass::Thai
            | ScriptClass::Han
            | ScriptClass::HiraganaKatakana
            | ScriptClass::Hangul => Some(TextDirection::LeftToRight),
            ScriptClass::Emoji | ScriptClass::Common | ScriptClass::Unknown => None,
        })
        .unwrap_or(TextDirection::LeftToRight)
}

#[allow(clippy::match_same_arms)]
fn classify_script(character: char) -> ScriptClass {
    match character as u32 {
        0x0041..=0x024F | 0x1E00..=0x1EFF => ScriptClass::Latin,
        0x0370..=0x03FF => ScriptClass::Greek,
        0x0400..=0x052F => ScriptClass::Cyrillic,
        0x0590..=0x05FF => ScriptClass::Hebrew,
        0x0600..=0x08FF | 0xFB50..=0xFDFF | 0xFE70..=0xFEFF => ScriptClass::Arabic,
        0x0900..=0x097F => ScriptClass::Devanagari,
        0x0980..=0x09FF => ScriptClass::Bengali,
        0x0E00..=0x0E7F => ScriptClass::Thai,
        0x3040..=0x30FF | 0x31F0..=0x31FF => ScriptClass::HiraganaKatakana,
        0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF => ScriptClass::Han,
        0x1100..=0x11FF | 0x3130..=0x318F | 0xAC00..=0xD7AF => ScriptClass::Hangul,
        0x1F000..=0x1FAFF | 0x2600..=0x27BF => ScriptClass::Emoji,
        0x0000..=0x0040 | 0x2000..=0x206F => ScriptClass::Common,
        _ => ScriptClass::Unknown,
    }
}
