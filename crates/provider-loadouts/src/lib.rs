//! Provider/model loadouts with deterministic scope inheritance.
//!
//! This crate deliberately contains no provider clients, catalog lookups, or
//! credential values. Callers pass catalog-derived disclosure metadata in and
//! store only opaque references to credentials held by a separate vault.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

pub const FORMAT: &str = "npc-provider-loadouts";
pub const SCHEMA_VERSION: u32 = 1;

const REQUIRED_ROLES: [ProviderRole; 4] = [
    ProviderRole::Llm,
    ProviderRole::Stt,
    ProviderRole::Tts,
    ProviderRole::Retrieval,
];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ProviderLoadoutDocumentV1 {
    pub format: String,
    pub schema_version: u32,
    pub loadouts: BTreeMap<LoadoutId, ProviderLoadoutV1>,
    pub activation: LoadoutActivationV1,
}

impl ProviderLoadoutDocumentV1 {
    pub fn new(default: ProviderLoadoutV1) -> Result<Self, LoadoutError> {
        let default_id = default.id.clone();
        if !matches!(default.scope, LoadoutScopeV1::Global) {
            return Err(LoadoutError::DefaultMustBeGlobal);
        }
        let document = Self {
            format: FORMAT.to_owned(),
            schema_version: SCHEMA_VERSION,
            loadouts: BTreeMap::from([(default_id.clone(), default)]),
            activation: LoadoutActivationV1 {
                global: default_id,
                games: BTreeMap::new(),
                characters: BTreeMap::new(),
            },
        };
        document.validate()?;
        Ok(document)
    }

    pub fn validate(&self) -> Result<(), LoadoutError> {
        if self.format != FORMAT {
            return Err(LoadoutError::UnsupportedFormat(self.format.clone()));
        }
        if self.schema_version != SCHEMA_VERSION {
            return Err(LoadoutError::UnsupportedSchemaVersion(self.schema_version));
        }
        if self.loadouts.is_empty() {
            return Err(LoadoutError::NoLoadouts);
        }

        let mut names = BTreeSet::new();
        for (key, loadout) in &self.loadouts {
            loadout.validate()?;
            if key != &loadout.id {
                return Err(LoadoutError::MapKeyMismatch {
                    key: key.clone(),
                    embedded: loadout.id.clone(),
                });
            }
            let name_key = (loadout.scope.clone(), normalized_name(&loadout.name));
            if !names.insert(name_key) {
                return Err(LoadoutError::DuplicateName {
                    scope: loadout.scope.clone(),
                    name: loadout.name.clone(),
                });
            }
        }

        for loadout in self.loadouts.values() {
            self.validate_parent(loadout)?;
            self.detect_cycle(&loadout.id)?;
        }
        self.validate_activation()?;
        self.resolve(&LoadoutContextV1::global(), &ValidationContextV1::online())?;
        for game_id in self.activation.games.keys() {
            self.resolve(
                &LoadoutContextV1::game(game_id),
                &ValidationContextV1::online(),
            )?;
        }
        for (game_id, characters) in &self.activation.characters {
            for character_id in characters.keys() {
                self.resolve(
                    &LoadoutContextV1::character(game_id, character_id),
                    &ValidationContextV1::online(),
                )?;
            }
        }
        Ok(())
    }

    pub fn insert(&mut self, loadout: ProviderLoadoutV1) -> Result<(), LoadoutError> {
        if self.loadouts.contains_key(&loadout.id) {
            return Err(LoadoutError::DuplicateId(loadout.id));
        }
        let id = loadout.id.clone();
        self.loadouts.insert(id.clone(), loadout);
        if let Err(error) = self.validate() {
            self.loadouts.remove(&id);
            return Err(error);
        }
        Ok(())
    }

    pub fn clone_loadout(
        &mut self,
        source: &LoadoutId,
        new_id: LoadoutId,
        new_name: String,
    ) -> Result<(), LoadoutError> {
        new_id.validate("loadout id")?;
        if self.loadouts.contains_key(&new_id) {
            return Err(LoadoutError::DuplicateId(new_id));
        }
        let mut cloned = self
            .loadouts
            .get(source)
            .cloned()
            .ok_or_else(|| LoadoutError::NotFound(source.clone()))?;
        cloned.id = new_id;
        cloned.name = new_name;
        self.insert(cloned)
    }

    pub fn rename(&mut self, id: &LoadoutId, new_name: String) -> Result<(), LoadoutError> {
        let old_name = self
            .loadouts
            .get(id)
            .ok_or_else(|| LoadoutError::NotFound(id.clone()))?
            .name
            .clone();
        self.loadouts.get_mut(id).expect("checked above").name = new_name;
        if let Err(error) = self.validate() {
            self.loadouts.get_mut(id).expect("checked above").name = old_name;
            return Err(error);
        }
        Ok(())
    }

    pub fn delete(&mut self, id: &LoadoutId) -> Result<ProviderLoadoutV1, LoadoutError> {
        if self.activation.references(id) {
            return Err(LoadoutError::ActiveLoadoutCannotBeDeleted(id.clone()));
        }
        if self
            .loadouts
            .values()
            .any(|candidate| candidate.parent.as_ref() == Some(id))
        {
            return Err(LoadoutError::ParentLoadoutCannotBeDeleted(id.clone()));
        }
        self.loadouts
            .remove(id)
            .ok_or_else(|| LoadoutError::NotFound(id.clone()))
    }

    pub fn activate(&mut self, id: &LoadoutId) -> Result<(), LoadoutError> {
        let loadout = self
            .loadouts
            .get(id)
            .ok_or_else(|| LoadoutError::NotFound(id.clone()))?;
        let previous = self.activation.clone();
        match &loadout.scope {
            LoadoutScopeV1::Global => self.activation.global = id.clone(),
            LoadoutScopeV1::Game { game_id } => {
                self.activation.games.insert(game_id.clone(), id.clone());
            }
            LoadoutScopeV1::Character {
                game_id,
                character_id,
            } => {
                self.activation
                    .characters
                    .entry(game_id.clone())
                    .or_default()
                    .insert(character_id.clone(), id.clone());
            }
        }
        if let Err(error) = self.validate() {
            self.activation = previous;
            return Err(error);
        }
        Ok(())
    }

    /// Removes a game or character activation so resolution falls back to the
    /// next less-specific scope. The global activation is always required.
    pub fn deactivate_scope(
        &mut self,
        scope: &LoadoutScopeV1,
    ) -> Result<Option<LoadoutId>, LoadoutError> {
        match scope {
            LoadoutScopeV1::Global => Err(LoadoutError::GlobalCannotBeDeactivated),
            LoadoutScopeV1::Game { game_id } => {
                validate_id(game_id, "game id")?;
                Ok(self.activation.games.remove(game_id))
            }
            LoadoutScopeV1::Character {
                game_id,
                character_id,
            } => {
                validate_id(game_id, "game id")?;
                validate_id(character_id, "character id")?;
                let (removed, empty) = self.activation.characters.get_mut(game_id).map_or(
                    (None, false),
                    |characters| {
                        let removed = characters.remove(character_id);
                        (removed, characters.is_empty())
                    },
                );
                if empty {
                    self.activation.characters.remove(game_id);
                }
                Ok(removed)
            }
        }
    }

    pub fn resolve(
        &self,
        context: &LoadoutContextV1,
        validation: &ValidationContextV1,
    ) -> Result<ResolvedProviderLoadoutV1, LoadoutError> {
        context.validate()?;
        let leaf = self.active_leaf(context)?;
        let mut chain = Vec::new();
        let mut current = Some(leaf);
        while let Some(id) = current {
            let loadout = self
                .loadouts
                .get(&id)
                .ok_or_else(|| LoadoutError::NotFound(id.clone()))?;
            chain.push(id);
            current = loadout.parent.clone();
        }
        chain.reverse();

        let mut roles = BTreeMap::new();
        for id in &chain {
            let loadout = self
                .loadouts
                .get(id)
                .ok_or_else(|| LoadoutError::NotFound(id.clone()))?;
            for (role, override_value) in &loadout.roles {
                match override_value {
                    RoleOverrideV1::Inherit => {}
                    RoleOverrideV1::Disabled => {
                        roles.remove(role);
                    }
                    RoleOverrideV1::Route(route) => {
                        roles.insert(*role, route.as_ref().clone());
                    }
                }
            }
        }

        for role in REQUIRED_ROLES {
            if !roles.contains_key(&role) {
                return Err(LoadoutError::RequiredRoleMissing(role));
            }
        }
        for (role, route) in &roles {
            route.validate_for(*role, validation)?;
        }

        Ok(ResolvedProviderLoadoutV1 {
            schema_version: SCHEMA_VERSION,
            leaf_loadout_id: chain.last().cloned().ok_or(LoadoutError::NoLoadouts)?,
            inheritance_chain: chain,
            roles,
        })
    }

    fn active_leaf(&self, context: &LoadoutContextV1) -> Result<LoadoutId, LoadoutError> {
        if let (Some(game_id), Some(character_id)) = (&context.game_id, &context.character_id) {
            if let Some(id) = self
                .activation
                .characters
                .get(game_id)
                .and_then(|characters| characters.get(character_id))
            {
                return Ok(id.clone());
            }
        }
        if let Some(game_id) = &context.game_id {
            if let Some(id) = self.activation.games.get(game_id) {
                return Ok(id.clone());
            }
        }
        Ok(self.activation.global.clone())
    }

    fn validate_parent(&self, loadout: &ProviderLoadoutV1) -> Result<(), LoadoutError> {
        match (&loadout.scope, &loadout.parent) {
            (LoadoutScopeV1::Global, None) => Ok(()),
            (LoadoutScopeV1::Global, Some(_)) => Err(LoadoutError::GlobalCannotInherit),
            (_, None) => Err(LoadoutError::ScopedLoadoutRequiresParent(
                loadout.id.clone(),
            )),
            (scope, Some(parent_id)) => {
                let parent = self
                    .loadouts
                    .get(parent_id)
                    .ok_or_else(|| LoadoutError::ParentNotFound(parent_id.clone()))?;
                if scope.can_inherit_from(&parent.scope) {
                    Ok(())
                } else {
                    Err(LoadoutError::InvalidParentScope {
                        child: scope.clone(),
                        parent: parent.scope.clone(),
                    })
                }
            }
        }
    }

    fn detect_cycle(&self, start: &LoadoutId) -> Result<(), LoadoutError> {
        let mut seen = BTreeSet::new();
        let mut current = Some(start.clone());
        while let Some(id) = current {
            if !seen.insert(id.clone()) {
                return Err(LoadoutError::InheritanceCycle(start.clone()));
            }
            current = self
                .loadouts
                .get(&id)
                .and_then(|entry| entry.parent.clone());
        }
        Ok(())
    }

    fn validate_activation(&self) -> Result<(), LoadoutError> {
        let global = self
            .loadouts
            .get(&self.activation.global)
            .ok_or_else(|| LoadoutError::NotFound(self.activation.global.clone()))?;
        if !matches!(global.scope, LoadoutScopeV1::Global) {
            return Err(LoadoutError::DefaultMustBeGlobal);
        }
        for (game_id, id) in &self.activation.games {
            let loadout = self
                .loadouts
                .get(id)
                .ok_or_else(|| LoadoutError::NotFound(id.clone()))?;
            if loadout.scope
                != (LoadoutScopeV1::Game {
                    game_id: game_id.clone(),
                })
            {
                return Err(LoadoutError::ActivationScopeMismatch(id.clone()));
            }
        }
        for (game_id, characters) in &self.activation.characters {
            validate_id(game_id, "activation game id")?;
            for (character_id, id) in characters {
                validate_id(character_id, "activation character id")?;
                let loadout = self
                    .loadouts
                    .get(id)
                    .ok_or_else(|| LoadoutError::NotFound(id.clone()))?;
                if loadout.scope
                    != (LoadoutScopeV1::Character {
                        game_id: game_id.clone(),
                        character_id: character_id.clone(),
                    })
                {
                    return Err(LoadoutError::ActivationScopeMismatch(id.clone()));
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ProviderLoadoutV1 {
    pub id: LoadoutId,
    pub name: String,
    pub scope: LoadoutScopeV1,
    pub parent: Option<LoadoutId>,
    pub roles: BTreeMap<ProviderRole, RoleOverrideV1>,
}

impl ProviderLoadoutV1 {
    pub fn validate(&self) -> Result<(), LoadoutError> {
        self.id.validate("loadout id")?;
        validate_display_name(&self.name)?;
        self.scope.validate()?;
        if let Some(parent) = &self.parent {
            parent.validate("parent loadout id")?;
        }
        for (role, override_value) in &self.roles {
            if let RoleOverrideV1::Route(route) = override_value {
                route.validate_shape(*role)?;
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum LoadoutScopeV1 {
    Global,
    Game {
        game_id: String,
    },
    Character {
        game_id: String,
        character_id: String,
    },
}

impl LoadoutScopeV1 {
    fn validate(&self) -> Result<(), LoadoutError> {
        match self {
            Self::Global => Ok(()),
            Self::Game { game_id } => validate_id(game_id, "game id"),
            Self::Character {
                game_id,
                character_id,
            } => {
                validate_id(game_id, "game id")?;
                validate_id(character_id, "character id")
            }
        }
    }

    fn can_inherit_from(&self, parent: &Self) -> bool {
        match (self, parent) {
            (Self::Game { .. }, Self::Global) => true,
            (
                Self::Character { game_id, .. },
                Self::Game {
                    game_id: parent_game,
                },
            ) => game_id == parent_game,
            _ => false,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum ProviderRole {
    Llm,
    Stt,
    Tts,
    Retrieval,
    Lipsync,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(
    tag = "mode",
    content = "route",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum RoleOverrideV1 {
    Inherit,
    Disabled,
    Route(Box<RoleRouteV1>),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RoleRouteV1 {
    pub primary: ProviderModelRouteV1,
    #[serde(default)]
    pub fallbacks: Vec<ExplicitFallbackV1>,
}

impl RoleRouteV1 {
    fn validate_shape(&self, role: ProviderRole) -> Result<(), LoadoutError> {
        self.primary.validate_shape()?;
        let mut identities = BTreeSet::from([self.primary.identity()]);
        for fallback in &self.fallbacks {
            fallback.validate_shape()?;
            if !identities.insert(fallback.route.identity()) {
                return Err(LoadoutError::DuplicateRoute(role));
            }
        }
        Ok(())
    }

    fn validate_for(
        &self,
        role: ProviderRole,
        context: &ValidationContextV1,
    ) -> Result<(), LoadoutError> {
        self.validate_shape(role)?;
        self.primary.validate_for(role, context)?;
        for fallback in &self.fallbacks {
            fallback.route.validate_for(role, context)?;
            if fallback.activation != FallbackActivationV1::ManualOnly || !fallback.user_authorized
            {
                return Err(LoadoutError::FallbackNotExplicit(role));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ProviderModelRouteV1 {
    pub provider_id: String,
    pub model_id: String,
    pub credential: Option<CredentialReferenceV1>,
    pub disclosure: CatalogDisclosureV1,
    /// Required for local lip-sync because it is a separately downloaded,
    /// optional model pack rather than a hidden default.
    #[serde(default)]
    pub explicit_user_selection: bool,
}

impl ProviderModelRouteV1 {
    fn identity(&self) -> (String, String) {
        (self.provider_id.clone(), self.model_id.clone())
    }

    fn validate_shape(&self) -> Result<(), LoadoutError> {
        validate_id(&self.provider_id, "provider id")?;
        validate_id(&self.model_id, "model id")?;
        if let Some(credential) = &self.credential {
            credential.validate()?;
            if credential.provider_id != self.provider_id {
                return Err(LoadoutError::CredentialProviderMismatch);
            }
        }
        self.disclosure.validate()
    }

    fn validate_for(
        &self,
        role: ProviderRole,
        context: &ValidationContextV1,
    ) -> Result<(), LoadoutError> {
        self.validate_shape()?;
        if context.offline && self.disclosure.egress != EgressClassV1::None {
            return Err(LoadoutError::CloudRouteForbiddenOffline {
                role,
                provider_id: self.provider_id.clone(),
            });
        }
        if role == ProviderRole::Lipsync
            && (self.disclosure.execution != ExecutionLocationV1::Local
                || self.disclosure.egress != EgressClassV1::None)
        {
            return Err(LoadoutError::LipsyncMustBeLocal);
        }
        if role == ProviderRole::Lipsync && !self.explicit_user_selection {
            return Err(LoadoutError::LipsyncRequiresExplicitSelection);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CredentialReferenceV1 {
    pub provider_id: String,
    /// Opaque vault record identifier. It is intentionally restricted to a
    /// short slug so a credential value cannot be smuggled into the document.
    pub reference_id: String,
}

impl CredentialReferenceV1 {
    fn validate(&self) -> Result<(), LoadoutError> {
        validate_id(&self.provider_id, "credential provider id")?;
        validate_opaque_reference(&self.reference_id)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CatalogDisclosureV1 {
    pub catalog_revision: u64,
    pub execution: ExecutionLocationV1,
    pub egress: EgressClassV1,
    pub privacy_summary: String,
    pub cost_summary: String,
    #[serde(default)]
    pub transmitted_data: BTreeSet<TransmittedDataV1>,
}

impl CatalogDisclosureV1 {
    fn validate(&self) -> Result<(), LoadoutError> {
        if self.catalog_revision == 0 {
            return Err(LoadoutError::InvalidCatalogRevision);
        }
        validate_summary(&self.privacy_summary, "privacy summary")?;
        validate_summary(&self.cost_summary, "cost summary")?;
        if self.egress == EgressClassV1::None && !self.transmitted_data.is_empty() {
            return Err(LoadoutError::LocalRouteTransmitsData);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionLocationV1 {
    Local,
    Hosted,
    ExternalLocal,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EgressClassV1 {
    None,
    ProviderCloud,
    UserConfiguredEndpoint,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum TransmittedDataV1 {
    Transcript,
    MicrophoneAudio,
    ResponseText,
    GameContext,
    MemoryContext,
    Image,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ExplicitFallbackV1 {
    pub route: ProviderModelRouteV1,
    pub activation: FallbackActivationV1,
    pub user_authorized: bool,
}

impl ExplicitFallbackV1 {
    fn validate_shape(&self) -> Result<(), LoadoutError> {
        self.route.validate_shape()
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FallbackActivationV1 {
    ManualOnly,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LoadoutActivationV1 {
    pub global: LoadoutId,
    #[serde(default)]
    pub games: BTreeMap<String, LoadoutId>,
    #[serde(default)]
    pub characters: BTreeMap<String, BTreeMap<String, LoadoutId>>,
}

impl LoadoutActivationV1 {
    fn references(&self, id: &LoadoutId) -> bool {
        &self.global == id
            || self.games.values().any(|candidate| candidate == id)
            || self
                .characters
                .values()
                .flat_map(BTreeMap::values)
                .any(|candidate| candidate == id)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LoadoutContextV1 {
    pub game_id: Option<String>,
    pub character_id: Option<String>,
}

impl LoadoutContextV1 {
    pub const fn global() -> Self {
        Self {
            game_id: None,
            character_id: None,
        }
    }

    pub fn game(game_id: impl Into<String>) -> Self {
        Self {
            game_id: Some(game_id.into()),
            character_id: None,
        }
    }

    pub fn character(game_id: impl Into<String>, character_id: impl Into<String>) -> Self {
        Self {
            game_id: Some(game_id.into()),
            character_id: Some(character_id.into()),
        }
    }

    fn validate(&self) -> Result<(), LoadoutError> {
        if self.character_id.is_some() && self.game_id.is_none() {
            return Err(LoadoutError::CharacterRequiresGame);
        }
        if let Some(game_id) = &self.game_id {
            validate_id(game_id, "game id")?;
        }
        if let Some(character_id) = &self.character_id {
            validate_id(character_id, "character id")?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ValidationContextV1 {
    pub offline: bool,
}

impl ValidationContextV1 {
    pub const fn online() -> Self {
        Self { offline: false }
    }

    pub const fn offline() -> Self {
        Self { offline: true }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ResolvedProviderLoadoutV1 {
    pub schema_version: u32,
    pub leaf_loadout_id: LoadoutId,
    pub inheritance_chain: Vec<LoadoutId>,
    pub roles: BTreeMap<ProviderRole, RoleRouteV1>,
}

impl ResolvedProviderLoadoutV1 {
    /// Captures an immutable route snapshot for a turn. A cancellation flag is
    /// state carried alongside the snapshot and cannot select another route.
    pub fn pin_turn_routes(&self, generation: u64) -> TurnRouteSnapshotV1 {
        TurnRouteSnapshotV1 {
            schema_version: SCHEMA_VERSION,
            source_loadout_id: self.leaf_loadout_id.clone(),
            generation,
            cancelled: false,
            selected: self
                .roles
                .iter()
                .map(|(role, route)| (*role, route.primary.clone()))
                .collect(),
        }
    }

    /// Returns an explicitly configured fallback as a candidate for a future
    /// turn. It never mutates an existing turn snapshot.
    pub fn manual_fallback_candidate(
        &self,
        role: ProviderRole,
        index: usize,
    ) -> Result<ProviderModelRouteV1, LoadoutError> {
        let fallback = self
            .roles
            .get(&role)
            .and_then(|route| route.fallbacks.get(index))
            .ok_or(LoadoutError::FallbackNotFound(role, index))?;
        if fallback.activation != FallbackActivationV1::ManualOnly || !fallback.user_authorized {
            return Err(LoadoutError::FallbackNotExplicit(role));
        }
        Ok(fallback.route.clone())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TurnRouteSnapshotV1 {
    pub schema_version: u32,
    pub source_loadout_id: LoadoutId,
    pub generation: u64,
    pub cancelled: bool,
    selected: BTreeMap<ProviderRole, ProviderModelRouteV1>,
}

impl TurnRouteSnapshotV1 {
    pub fn route(&self, role: ProviderRole) -> Option<&ProviderModelRouteV1> {
        self.selected.get(&role)
    }

    pub fn cancel(&mut self) {
        self.cancelled = true;
    }

    pub fn automatic_fallback(&self, role: ProviderRole) -> Result<(), LoadoutError> {
        Err(LoadoutError::AutomaticFallbackForbidden(role))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(transparent)]
pub struct LoadoutId(pub String);

impl LoadoutId {
    pub fn new(value: impl Into<String>) -> Result<Self, LoadoutError> {
        let id = Self(value.into());
        id.validate("loadout id")?;
        Ok(id)
    }

    fn validate(&self, field: &'static str) -> Result<(), LoadoutError> {
        validate_id(&self.0, field)
    }
}

impl std::fmt::Display for LoadoutId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum LoadoutError {
    #[error("unsupported loadout format: {0}")]
    UnsupportedFormat(String),
    #[error("unsupported loadout schema version: {0}")]
    UnsupportedSchemaVersion(u32),
    #[error("the document contains no loadouts")]
    NoLoadouts,
    #[error("the default loadout must have global scope")]
    DefaultMustBeGlobal,
    #[error("invalid {field}: {value}")]
    InvalidId { field: &'static str, value: String },
    #[error("invalid loadout display name")]
    InvalidDisplayName,
    #[error("invalid {0}")]
    InvalidSummary(&'static str),
    #[error("invalid credential reference")]
    InvalidCredentialReference,
    #[error("credential reference provider does not match route provider")]
    CredentialProviderMismatch,
    #[error("catalog revision must be nonzero")]
    InvalidCatalogRevision,
    #[error("a local route cannot declare transmitted data")]
    LocalRouteTransmitsData,
    #[error("duplicate loadout id: {0}")]
    DuplicateId(LoadoutId),
    #[error("duplicate loadout name {name:?} in scope {scope:?}")]
    DuplicateName { scope: LoadoutScopeV1, name: String },
    #[error("loadout map key {key} does not match embedded id {embedded}")]
    MapKeyMismatch { key: LoadoutId, embedded: LoadoutId },
    #[error("loadout not found: {0}")]
    NotFound(LoadoutId),
    #[error("parent loadout not found: {0}")]
    ParentNotFound(LoadoutId),
    #[error("global loadouts cannot inherit")]
    GlobalCannotInherit,
    #[error("scoped loadout requires a parent: {0}")]
    ScopedLoadoutRequiresParent(LoadoutId),
    #[error("invalid parent scope {parent:?} for child scope {child:?}")]
    InvalidParentScope {
        child: LoadoutScopeV1,
        parent: LoadoutScopeV1,
    },
    #[error("inheritance cycle detected from {0}")]
    InheritanceCycle(LoadoutId),
    #[error("activation scope does not match loadout: {0}")]
    ActivationScopeMismatch(LoadoutId),
    #[error("active loadout cannot be deleted: {0}")]
    ActiveLoadoutCannotBeDeleted(LoadoutId),
    #[error("the global loadout activation cannot be removed")]
    GlobalCannotBeDeactivated,
    #[error("loadout used as a parent cannot be deleted: {0}")]
    ParentLoadoutCannotBeDeleted(LoadoutId),
    #[error("character context requires a game")]
    CharacterRequiresGame,
    #[error("required provider role is missing after inheritance: {0:?}")]
    RequiredRoleMissing(ProviderRole),
    #[error("duplicate provider/model route for role {0:?}")]
    DuplicateRoute(ProviderRole),
    #[error("fallback for role {0:?} is not explicitly authorized and manual-only")]
    FallbackNotExplicit(ProviderRole),
    #[error("fallback {1} for role {0:?} was not found")]
    FallbackNotFound(ProviderRole, usize),
    #[error("automatic fallback is forbidden for role {0:?}")]
    AutomaticFallbackForbidden(ProviderRole),
    #[error("cloud route {provider_id} for role {role:?} is forbidden offline")]
    CloudRouteForbiddenOffline {
        role: ProviderRole,
        provider_id: String,
    },
    #[error("lip-sync routes must be local and have no egress")]
    LipsyncMustBeLocal,
    #[error("lip-sync packs require explicit user selection")]
    LipsyncRequiresExplicitSelection,
}

fn normalized_name(value: &str) -> String {
    value.trim().to_lowercase()
}

fn validate_display_name(value: &str) -> Result<(), LoadoutError> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.len() > 80 || trimmed.chars().any(char::is_control) {
        return Err(LoadoutError::InvalidDisplayName);
    }
    Ok(())
}

fn validate_summary(value: &str, field: &'static str) -> Result<(), LoadoutError> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.len() > 500 || trimmed.chars().any(char::is_control) {
        return Err(LoadoutError::InvalidSummary(field));
    }
    Ok(())
}

fn validate_id(value: &str, field: &'static str) -> Result<(), LoadoutError> {
    let length_ok = !value.is_empty() && value.len() <= 128;
    let starts_ok = value
        .bytes()
        .next()
        .is_some_and(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit());
    let chars_ok = value.bytes().all(|byte| {
        byte.is_ascii_lowercase()
            || byte.is_ascii_digit()
            || matches!(byte, b'-' | b'_' | b'.' | b'/')
    });
    if length_ok && starts_ok && chars_ok {
        Ok(())
    } else {
        Err(LoadoutError::InvalidId {
            field,
            value: value.to_owned(),
        })
    }
}

fn validate_opaque_reference(value: &str) -> Result<(), LoadoutError> {
    let valid = !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_' | b'.')
        });
    if valid {
        Ok(())
    } else {
        Err(LoadoutError::InvalidCredentialReference)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(value: &str) -> LoadoutId {
        LoadoutId::new(value).expect("test id is valid")
    }

    fn disclosure(execution: ExecutionLocationV1, egress: EgressClassV1) -> CatalogDisclosureV1 {
        CatalogDisclosureV1 {
            catalog_revision: 5,
            execution,
            egress,
            privacy_summary: "Catalog supplied privacy statement.".to_owned(),
            cost_summary: "Catalog supplied cost statement.".to_owned(),
            transmitted_data: if egress == EgressClassV1::None {
                BTreeSet::new()
            } else {
                BTreeSet::from([TransmittedDataV1::Transcript])
            },
        }
    }

    fn route(provider: &str, model: &str, local: bool) -> ProviderModelRouteV1 {
        ProviderModelRouteV1 {
            provider_id: provider.to_owned(),
            model_id: model.to_owned(),
            credential: (!local).then(|| CredentialReferenceV1 {
                provider_id: provider.to_owned(),
                reference_id: "personal".to_owned(),
            }),
            disclosure: if local {
                disclosure(ExecutionLocationV1::Local, EgressClassV1::None)
            } else {
                disclosure(ExecutionLocationV1::Hosted, EgressClassV1::ProviderCloud)
            },
            explicit_user_selection: false,
        }
    }

    fn role_route(provider: &str, model: &str, local: bool) -> RoleOverrideV1 {
        RoleOverrideV1::Route(Box::new(RoleRouteV1 {
            primary: route(provider, model, local),
            fallbacks: Vec::new(),
        }))
    }

    fn global_loadout() -> ProviderLoadoutV1 {
        ProviderLoadoutV1 {
            id: id("global-balanced"),
            name: "Balanced".to_owned(),
            scope: LoadoutScopeV1::Global,
            parent: None,
            roles: BTreeMap::from([
                (ProviderRole::Llm, role_route("openai", "gpt-5", false)),
                (ProviderRole::Stt, role_route("deepgram", "nova-3", false)),
                (
                    ProviderRole::Tts,
                    role_route("elevenlabs", "flash-v2.5", false),
                ),
                (
                    ProviderRole::Retrieval,
                    role_route("local-onnx", "bge-small-en", true),
                ),
            ]),
        }
    }

    fn document() -> ProviderLoadoutDocumentV1 {
        ProviderLoadoutDocumentV1::new(global_loadout()).expect("valid fixture")
    }

    #[test]
    fn rejects_invalid_ids_and_duplicate_names_case_insensitively() {
        assert!(matches!(
            LoadoutId::new("Not Valid"),
            Err(LoadoutError::InvalidId { .. })
        ));
        let mut document = document();
        let duplicate = ProviderLoadoutV1 {
            id: id("global-other"),
            name: " balanced ".to_owned(),
            ..global_loadout()
        };
        assert!(matches!(
            document.insert(duplicate),
            Err(LoadoutError::DuplicateName { .. })
        ));
        assert_eq!(document.loadouts.len(), 1);
    }

    #[test]
    fn resolves_global_game_character_overrides_deterministically() {
        let mut document = document();
        let game = ProviderLoadoutV1 {
            id: id("cyberpunk-fast"),
            name: "Cyberpunk fast".to_owned(),
            scope: LoadoutScopeV1::Game {
                game_id: "cyberpunk-2077".to_owned(),
            },
            parent: Some(id("global-balanced")),
            roles: BTreeMap::from([(
                ProviderRole::Llm,
                role_route("groq", "llama-3.3-70b", false),
            )]),
        };
        document.insert(game).expect("game insert");
        let character = ProviderLoadoutV1 {
            id: id("judy-voice"),
            name: "Judy voice".to_owned(),
            scope: LoadoutScopeV1::Character {
                game_id: "cyberpunk-2077".to_owned(),
                character_id: "judy-alvarez".to_owned(),
            },
            parent: Some(id("cyberpunk-fast")),
            roles: BTreeMap::from([(ProviderRole::Tts, role_route("cartesia", "sonic-3", false))]),
        };
        document.insert(character).expect("character insert");
        document
            .activate(&id("cyberpunk-fast"))
            .expect("activate game");
        document
            .activate(&id("judy-voice"))
            .expect("activate character");

        let resolved = document
            .resolve(
                &LoadoutContextV1::character("cyberpunk-2077", "judy-alvarez"),
                &ValidationContextV1::online(),
            )
            .expect("resolve");
        assert_eq!(
            resolved.inheritance_chain,
            vec![
                id("global-balanced"),
                id("cyberpunk-fast"),
                id("judy-voice")
            ]
        );
        assert_eq!(
            resolved.roles[&ProviderRole::Llm].primary.provider_id,
            "groq"
        );
        assert_eq!(
            resolved.roles[&ProviderRole::Tts].primary.provider_id,
            "cartesia"
        );
        assert_eq!(
            resolved.roles[&ProviderRole::Stt].primary.provider_id,
            "deepgram"
        );
    }

    #[test]
    fn disabled_required_role_is_rejected_after_merge() {
        let mut document = document();
        let game = ProviderLoadoutV1 {
            id: id("game-no-stt"),
            name: "No STT".to_owned(),
            scope: LoadoutScopeV1::Game {
                game_id: "skyrim-se".to_owned(),
            },
            parent: Some(id("global-balanced")),
            roles: BTreeMap::from([(ProviderRole::Stt, RoleOverrideV1::Disabled)]),
        };
        document.insert(game).expect("insert is structurally valid");
        assert!(matches!(
            document.activate(&id("game-no-stt")),
            Err(LoadoutError::RequiredRoleMissing(ProviderRole::Stt))
        ));
    }

    #[test]
    fn serialization_has_no_credential_value_and_rejects_secret_fields() {
        let document = document();
        let json = serde_json::to_string(&document).expect("serialize");
        assert!(!json.contains("api_key"));
        assert!(!json.contains("secret"));
        assert!(json.contains("reference_id"));

        let malicious = json.replacen(
            "\"reference_id\":\"personal\"",
            "\"reference_id\":\"personal\",\"api_key\":\"nvapi-example-secret\"",
            1,
        );
        assert!(serde_json::from_str::<ProviderLoadoutDocumentV1>(&malicious).is_err());
        assert!(matches!(
            validate_opaque_reference(
                "nvapi-abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789"
            ),
            Err(LoadoutError::InvalidCredentialReference)
        ));
    }

    #[test]
    fn fallback_requires_authorization_and_is_never_automatic() {
        let mut global = global_loadout();
        let RoleOverrideV1::Route(llm) =
            global.roles.get_mut(&ProviderRole::Llm).expect("llm role")
        else {
            panic!("expected route")
        };
        llm.fallbacks.push(ExplicitFallbackV1 {
            route: route("anthropic", "claude-sonnet", false),
            activation: FallbackActivationV1::ManualOnly,
            user_authorized: false,
        });
        assert!(matches!(
            ProviderLoadoutDocumentV1::new(global.clone()),
            Err(LoadoutError::FallbackNotExplicit(ProviderRole::Llm))
        ));

        let RoleOverrideV1::Route(llm) =
            global.roles.get_mut(&ProviderRole::Llm).expect("llm role")
        else {
            panic!("expected route")
        };
        llm.fallbacks[0].user_authorized = true;
        let resolved = ProviderLoadoutDocumentV1::new(global)
            .expect("authorized config")
            .resolve(&LoadoutContextV1::global(), &ValidationContextV1::online())
            .expect("resolve");
        assert_eq!(
            resolved
                .manual_fallback_candidate(ProviderRole::Llm, 0)
                .expect("manual fallback")
                .provider_id,
            "anthropic"
        );
        let snapshot = resolved.pin_turn_routes(9);
        assert_eq!(
            snapshot
                .route(ProviderRole::Llm)
                .expect("route")
                .provider_id,
            "openai"
        );
        assert_eq!(
            snapshot.automatic_fallback(ProviderRole::Llm),
            Err(LoadoutError::AutomaticFallbackForbidden(ProviderRole::Llm))
        );
    }

    #[test]
    fn cancellation_cannot_change_pinned_routes() {
        let resolved = document()
            .resolve(&LoadoutContextV1::global(), &ValidationContextV1::online())
            .expect("resolve");
        let mut snapshot = resolved.pin_turn_routes(17);
        let before = snapshot.route(ProviderRole::Llm).cloned();
        snapshot.cancel();
        assert!(snapshot.cancelled);
        assert_eq!(snapshot.generation, 17);
        assert_eq!(snapshot.route(ProviderRole::Llm), before.as_ref());
    }

    #[test]
    fn offline_context_rejects_primary_and_fallback_cloud_routes() {
        assert!(matches!(
            document().resolve(&LoadoutContextV1::global(), &ValidationContextV1::offline()),
            Err(LoadoutError::CloudRouteForbiddenOffline { .. })
        ));

        let mut local = global_loadout();
        for role in [ProviderRole::Llm, ProviderRole::Stt, ProviderRole::Tts] {
            local
                .roles
                .insert(role, role_route("local-runtime", "local-model", true));
        }
        let RoleOverrideV1::Route(llm) = local.roles.get_mut(&ProviderRole::Llm).expect("llm")
        else {
            panic!("expected route")
        };
        llm.fallbacks.push(ExplicitFallbackV1 {
            route: route("openai", "gpt-5", false),
            activation: FallbackActivationV1::ManualOnly,
            user_authorized: true,
        });
        let document = ProviderLoadoutDocumentV1::new(local).expect("online-valid document");
        assert!(matches!(
            document.resolve(&LoadoutContextV1::global(), &ValidationContextV1::offline()),
            Err(LoadoutError::CloudRouteForbiddenOffline {
                role: ProviderRole::Llm,
                ..
            })
        ));
    }

    #[test]
    fn lipsync_is_optional_local_and_explicitly_selected() {
        let mut global = global_loadout();
        let mut lipsync = route("local-pack", "musetalk-1.5", true);
        global.roles.insert(
            ProviderRole::Lipsync,
            RoleOverrideV1::Route(Box::new(RoleRouteV1 {
                primary: lipsync.clone(),
                fallbacks: Vec::new(),
            })),
        );
        assert!(matches!(
            ProviderLoadoutDocumentV1::new(global.clone()),
            Err(LoadoutError::LipsyncRequiresExplicitSelection)
        ));

        lipsync.explicit_user_selection = true;
        global.roles.insert(
            ProviderRole::Lipsync,
            RoleOverrideV1::Route(Box::new(RoleRouteV1 {
                primary: lipsync,
                fallbacks: Vec::new(),
            })),
        );
        ProviderLoadoutDocumentV1::new(global).expect("explicit local lip-sync is valid");

        let mut hosted = global_loadout();
        let mut hosted_lipsync = route("hosted-video", "talking-head", false);
        hosted_lipsync.explicit_user_selection = true;
        hosted.roles.insert(
            ProviderRole::Lipsync,
            RoleOverrideV1::Route(Box::new(RoleRouteV1 {
                primary: hosted_lipsync,
                fallbacks: Vec::new(),
            })),
        );
        assert!(matches!(
            ProviderLoadoutDocumentV1::new(hosted),
            Err(LoadoutError::LipsyncMustBeLocal)
        ));
    }

    #[test]
    fn clone_rename_delete_and_activation_are_transactional() {
        let mut document = document();
        document
            .clone_loadout(
                &id("global-balanced"),
                id("global-quality"),
                "Maximum quality".to_owned(),
            )
            .expect("clone");
        document
            .rename(&id("global-quality"), "Quality".to_owned())
            .expect("rename");
        document.activate(&id("global-quality")).expect("activate");
        assert!(matches!(
            document.delete(&id("global-quality")),
            Err(LoadoutError::ActiveLoadoutCannotBeDeleted(_))
        ));
        document
            .activate(&id("global-balanced"))
            .expect("reactivate");
        let removed = document.delete(&id("global-quality")).expect("delete");
        assert_eq!(removed.name, "Quality");
    }

    #[test]
    fn scoped_activation_can_be_cleared_before_safe_deletion() {
        let mut document = document();
        let game = ProviderLoadoutV1 {
            id: id("skyrim-local"),
            name: "Skyrim local".to_owned(),
            scope: LoadoutScopeV1::Game {
                game_id: "skyrim-se".to_owned(),
            },
            parent: Some(id("global-balanced")),
            roles: BTreeMap::new(),
        };
        document.insert(game).expect("insert");
        document.activate(&id("skyrim-local")).expect("activate");
        let removed_activation = document
            .deactivate_scope(&LoadoutScopeV1::Game {
                game_id: "skyrim-se".to_owned(),
            })
            .expect("deactivate");
        assert_eq!(removed_activation, Some(id("skyrim-local")));
        document.delete(&id("skyrim-local")).expect("delete");
        assert_eq!(
            document.deactivate_scope(&LoadoutScopeV1::Global),
            Err(LoadoutError::GlobalCannotBeDeactivated)
        );
    }
}
