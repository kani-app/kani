//! Source host management for WASM extensions.

use crate::error::{Error, Result};
use crate::wasm::{AllowedHost, HostState};
use std::sync::Arc;
use wasmtime::Store;
use wasmtime::component::Linker;

pub const EPOCH_DEADLINE_TICKS: u64 = 500;

#[macro_export]
macro_rules! execute_wasm {
    ($self:expr, $method:ident $(, $args:expr)*) => {{
        {
            let data = $self.store.data_mut();
            data.begin_operation();
        }

        $self.store.set_epoch_deadline($crate::sources::EPOCH_DEADLINE_TICKS);
        let provider = $self.bindings.kani_extension_manga_provider();
        let raw_result = provider.$method(&mut $self.store $(, $args)*)
            .await
            .map_err(|e| {
                tracing::error!(target: "wasm", "trap in {}: {e:#}", stringify!($method));
                $crate::error::Error::Internal(format!("WASM function call failed: {e:#}"))
            });

        $self.store.data_mut().clear_all();
        let inner = raw_result?;
        let result = inner.map_err($crate::wasm::guest_error)?;
        Ok(result)
    }};
}

/// Hosts a single WASM source extension. Used for one-off instantiation
pub struct SourceInstance {
    pub store: Option<Store<HostState>>,
    pub bindings: Option<crate::wasm::KaniExtension>,
    smart_client: crate::http::SmartClient,
    base_url: Option<String>,
    unrestricted_http: bool,
}

impl SourceInstance {
    pub fn new(
        smart_client: crate::http::SmartClient,
        base_url: Option<String>,
        unrestricted_http: bool,
    ) -> Self {
        Self {
            store: None,
            bindings: None,
            smart_client,
            base_url,
            unrestricted_http,
        }
    }

    /// Loads a source from the component.
    pub async fn load(
        &mut self,
        engine: &wasmtime::Engine,
        component: &wasmtime::component::Component,
        linker: &Linker<HostState>,
    ) -> Result<()> {
        let allowed_host = match (self.base_url.as_deref(), self.unrestricted_http) {
            (_, true) => AllowedHost::Unrestricted,
            (Some(url), false) => AllowedHost::Restricted(url.to_string()),
            (None, false) => AllowedHost::MetadataOnly,
        };

        let mut store = Store::try_new(
            engine,
            HostState::new(
                self.smart_client.clone(),
                allowed_host,
                Arc::new(crate::cache::InMemoryCache::new()),
                String::new(),
                crate::v8_process::new_handle(),
            )?,
        )?;

        store.set_epoch_deadline(EPOCH_DEADLINE_TICKS);
        store.epoch_deadline_callback(|mut ctx| {
            let data = ctx.data_mut();
            if data
                .last_io_at
                .map(|t| t.elapsed().as_millis() < (EPOCH_DEADLINE_TICKS * 10) as u128)
                .unwrap_or(false)
            {
                data.last_io_at = None;
                Ok(wasmtime::UpdateDeadline::Continue(EPOCH_DEADLINE_TICKS))
            } else {
                Err(Error::Internal("WASM computation deadline exceeded".to_string()).into())
            }
        });

        let bindings = crate::wasm::KaniExtension::instantiate_async(&mut store, component, linker)
            .await
            .map_err(|e| Error::Internal(format!("Failed to instantiate: {}", e)))?;

        self.store = Some(store);
        self.bindings = Some(bindings);

        Ok(())
    }

    /// Injects current preference values into the store.
    pub fn set_preference_map(&mut self, prefs: std::collections::HashMap<String, String>) {
        if let Some(store) = &mut self.store {
            store.data_mut().preferences = prefs;
        }
    }

    /// Calls `get_fetched_option_sets`. Returns a JSON-encoded list of
    /// `kani_shared::FilterFetchDef`.
    pub async fn get_fetched_option_sets(&mut self) -> Result<String> {
        let store = self
            .store
            .as_mut()
            .ok_or_else(|| Error::Internal("Store not initialized".to_string()))?;
        let bindings = self
            .bindings
            .as_ref()
            .expect("Bindings should be initialized");

        {
            let data = store.data_mut();
            data.begin_operation();
        }

        store.set_epoch_deadline(EPOCH_DEADLINE_TICKS);
        let provider = bindings.kani_extension_manga_provider();
        let raw_result = provider
            .call_get_fetched_option_sets(&mut *store)
            .await
            .map_err(|e| {
                tracing::error!(target: "wasm", "trap in call_get_fetched_option_sets: {e:#}");
                Error::Internal(format!("WASM function call failed: {e:#}"))
            });

        store.data_mut().clear_all();
        let inner = raw_result?;
        inner.map_err(crate::wasm::guest_error)
    }

    /// Calls the `get_metadata` function in the WASM module. Returns the
    /// JSON-encoded `kani_shared::ExtensionMetadata` string.
    pub async fn get_metadata(&mut self) -> Result<String> {
        let store = self
            .store
            .as_mut()
            .ok_or_else(|| Error::Internal("Store not initialized".to_string()))?;
        let bindings = self
            .bindings
            .as_ref()
            .expect("Bindings should be initialized");

        {
            let data = store.data_mut();
            data.begin_operation();
        }

        store.set_epoch_deadline(EPOCH_DEADLINE_TICKS);
        let provider = bindings.kani_extension_manga_provider();
        let raw_result = provider.call_get_metadata(&mut *store).await.map_err(|e| {
            tracing::error!(target: "wasm", "trap in call_get_metadata: {e:#}");
            Error::Internal(format!("WASM function call failed: {e:#}"))
        });

        store.data_mut().clear_all();
        let inner = raw_result?;
        inner.map_err(crate::wasm::guest_error)
    }

    /// Calls the `get_preferences` function in the WASM module.
    pub async fn get_preferences(
        &mut self,
    ) -> Result<Vec<crate::wasm::kani::extension::types::PreferenceSpec>> {
        let store = self
            .store
            .as_mut()
            .ok_or_else(|| Error::Internal("Store not initialized".to_string()))?;
        let bindings = self
            .bindings
            .as_ref()
            .expect("Bindings should be initialized");

        {
            let data = store.data_mut();
            data.begin_operation();
        }

        store.set_epoch_deadline(EPOCH_DEADLINE_TICKS);
        let provider = bindings.kani_extension_manga_provider();
        let raw_result = provider
            .call_get_preferences(&mut *store)
            .await
            .map_err(|e| {
                tracing::error!(target: "wasm", "trap in call_get_preferences: {e:#}");
                Error::Internal(format!("WASM function call failed: {e:#}"))
            });

        store.data_mut().clear_all();
        let inner = raw_result?;
        inner.map_err(crate::wasm::guest_error)
    }
}
