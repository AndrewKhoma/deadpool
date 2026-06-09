/// Pool storage mode.
///
/// Existing constructors and builders use [`PoolMode::Shared`] unless another
/// mode is selected explicitly.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub enum PoolMode {
    /// Use the existing shared pool storage.
    #[default]
    Shared,
    /// Use explicit core-local pool handles sharing global capacity.
    #[cfg(feature = "core-local")]
    #[cfg_attr(docsrs, doc(cfg(feature = "core-local")))]
    CoreLocal,
}
