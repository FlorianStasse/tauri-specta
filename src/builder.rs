use std::{
    borrow::Cow,
    collections::HashMap,
    fs::{self, File},
    io::Write,
};

use serde::Serialize;
use specta::{datatype::DataType, NamedType, Type, TypeMap};
use specta_typescript::Typescript;
use tauri::{ipc::Invoke, App, Runtime};

use crate::{
    internal::{Commands, Events},
    EventRegistry,
};

pub struct Builder<R: Runtime> {
    plugin_name: Option<&'static str>,
    commands: Commands<R>,
    events: Events,
    types: TypeMap,
    constants: HashMap<Cow<'static, str>, (DataType, serde_json::Value)>,
}

impl<R: Runtime> Default for Builder<R> {
    fn default() -> Self {
        Self {
            plugin_name: None,
            commands: Commands::default(),
            events: Events::default(),
            types: TypeMap::default(),
            constants: HashMap::default(),
        }
    }
}

impl<R: Runtime> Builder<R> {
    /// Construct a new Tauri Specta builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the name of the current plugin name.
    ///
    /// This is used to ensure the generated bindings correctly reference the plugin.
    pub fn plugin_name(self, plugin_name: &'static str) -> Self {
        Self {
            plugin_name: Some(plugin_name),
            ..self
        }
    }

    /// Register commands with the builder.
    ///
    /// WARNING: This method will overwrite any previously registered commands.
    pub fn commands(self, commands: Commands<R>) -> Self {
        Self { commands, ..self }
    }

    /// Register events with the builder.
    ///
    /// WARNING: This method will overwrite any previously registered events.
    pub fn events(self, events: Events) -> Self {
        Self { events, ..self }
    }
    /// Export a new type with the frontend.
    ///
    /// This is useful if you want to export types that do not appear in any events or commands.
    pub fn ty<T: NamedType>(mut self) -> Self {
        let dt = T::definition_named_data_type(&mut self.types);
        self.types.insert(T::sid(), dt);
        self
    }

    /// Export a constant value to the frontend.
    ///
    /// This is useful to share application-wide constants or expose data which is generated by Rust.
    #[track_caller]
    pub fn constant<T: Serialize + Type>(mut self, k: impl Into<Cow<'static, str>>, v: T) -> Self {
        let v = serde_json::to_value(v).expect("Tauri Specta failed to serialize constant");
        self.constants
            .insert(k.into(), (T::reference(&mut self.types, &[]).inner, v));
        self
    }

    // TODO: Maybe method to merge in a `TypeCollection`

    // TODO: Should we put a `.build` command here to ensure it's immutable from now on?

    /// The Tauri invoke handler to trigger commands registered with the builder.
    pub fn invoke_handler(&self) -> impl Fn(Invoke<R>) -> bool + Send + Sync + 'static {
        let commands = self.commands.0.clone();
        move |invoke| commands(invoke)
    }

    /// Mount all of the events in the builder onto a Tauri app.
    pub fn mount_events(&self, app: &mut App<R>) {
        let registry = EventRegistry::get_or_manage(app);
        registry.register_collection(self.events.0.clone(), None);
    }

    // TODO: Restructure to use a `LanguageExt` trait system

    // TODO: Make this not-mutable
    pub fn export_ts(
        &mut self,
        language: Typescript,
    ) -> Result<(), specta_typescript::ExportError> {
        if let Some(path) = &language.path {
            if let Some(export_dir) = path.parent() {
                fs::create_dir_all(export_dir)?;
            }

            let mut file = File::create(&path)?;

            // TODO: Maybe do this in the `commands` to make sure this method can take `&self`
            let commands = (self.commands.1)(&mut self.types);

            // TODO: This is required for channels to work correctly.
            // This should be unfeature gated once the upstream fix is merged: https://github.com/tauri-apps/tauri/pull/10435
            // #[cfg(feature = "UNSTABLE_channels")]
            // self.types
            //     .remove(<tauri::ipc::Channel<()> as specta::NamedType>::sid());

            let dependant_types = self
                .types
                .iter()
                .map({
                    let language = &language;
                    |(_sid, ndt)| {
                        specta_typescript::export_named_datatype(language, ndt, &self.types)
                    }
                })
                .collect::<Result<Vec<_>, _>>()
                .map(|v| v.join("\n"))?;

            let rendered = crate::js_ts::render_all_parts::<specta_typescript::Typescript>(
                &commands,
                &self.events.1,
                &self.types,
                &Default::default(), // TODO: fix statics
                &language,
                &self.plugin_name,
                &dependant_types,
                crate::ts::GLOBALS,
            )?;

            write!(file, "{}", format!("{}\n{rendered}", language.header))?;

            language.run_format(path.clone()).ok();
        }

        Ok(())
    }

    pub fn export_js_doc(&self, language: Typescript) {
        todo!();
    }
}
