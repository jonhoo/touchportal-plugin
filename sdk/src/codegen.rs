use crate::{
    ActionImplementation, ChoiceSetting, Data, DataFormat, PluginDescription, SettingType,
};

use indexmap::IndexMap;
use inflector::Inflector;
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use std::collections::BTreeSet;
use syn::Ident;

#[allow(clippy::needless_doctest_main)]
/// Generates the binding code for your plugin and exports it to `$OUT_DIR`.
///
/// This is the recommended way to handle plugin build outputs in your `build.rs`:
///
/// ```rust,no_run
/// use touchportal_sdk::{PluginDescription, codegen};
/// fn main() {
///     let plugin = PluginDescription::builder()
///       /* build your plugin manifest here */
///       .build()
///       .unwrap();
///
///     // Generate and write all build outputs
///     codegen::export(&plugin);
/// }
/// ```
///
/// The generated Rust code will go to `$OUT_DIR/entry.rs`, which you should then include form your
/// plugin's `main.rs`, like so:
///
/// ```rust,ignore
/// include!(concat!(env!("OUT_DIR"), "/entry.rs"));
///
/// #[derive(Debug)]
/// struct Plugin(TouchPortalHandle);
///
/// impl Plugin {
///     async fn new(
///         settings: PluginSettings,
///         outgoing: TouchPortalHandle,
///         info: InfoMessage,
///     ) -> eyre::Result<Self> {
///         Ok(Self(outgoing))
///     }
/// }
///
/// impl PluginCallbacks for Plugin {
///     // your IDE/the compiler errors will guide you here
/// }
///
/// #[tokio::main]
/// async fn main() -> eyre::Result<()> {
///     Plugin::run_dynamic("127.0.0.1:12136").await
/// }
/// ```
///
/// Internally, this is a combination of [`generate`] and JSON-serializing `plugin` to generate an
/// `entry.tp` file, both of which end up in `$OUT_DIR`. That is, it's roughly equivalent to:
///
/// ```rust,no_run
/// # use touchportal_sdk::{PluginDescription, codegen};
/// # let plugin = PluginDescription::builder().build().unwrap();
///
/// // write out generated code to somewhere your main.rs can include! it from:
/// std::fs::write(
///     format!("{}/entry.rs", std::env::var("OUT_DIR").unwrap()),
///     codegen::generate(&plugin),
/// )
/// .unwrap();
///
/// // also write out your serialized plugin manifest (`entry.tp`) to the same place:
/// std::fs::write(
///     format!("{}/entry.tp", std::env::var("OUT_DIR").unwrap()),
///     serde_json::to_vec(&plugin).unwrap(),
/// )
/// .unwrap();
/// ```
///
/// Note that `package.py` (and thus also `install.py`) expect to find the `entry.tp` file
/// `$OUT_DIR`.
pub fn export(plugin: &PluginDescription) {
    // Generate the Rust binding code
    let rust_code = generate(plugin);

    // Get the OUT_DIR environment variable
    let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR environment variable not set");

    // Write the generated Rust code
    std::fs::write(format!("{}/entry.rs", out_dir), rust_code)
        .expect("write generated Rust code to OUT_DIR/entry.rs");

    // Write the serialized plugin description
    std::fs::write(
        format!("{}/entry.tp", out_dir),
        serde_json::to_vec(plugin).expect("serialize plugin description"),
    )
    .expect("write serialized plugin description to OUT_DIR/entry.tp");

    // Output cargo directives for proper rebuild detection
    println!("cargo::rerun-if-changed=build.rs");
}

/// Generates the Rust binding code for your plugin and returns it.
///
/// You'll generally want to put this code in `$OUT_DIR` so you can `include!` it from your
/// plugin's `main.rs`.
///
/// Prefer using [`export`].
pub fn generate(plugin: &PluginDescription) -> String {
    // also write out &'static PluginDescription
    // defs probably go to lib, and so does the static (const?) construction of the instance.
    // then, this loads that to make entry.tp _and_ it's used to codegen (how?) action+event bindings.
    // maybe actually there is a crate that has these impls that's then used as a build dep of the main
    // crate?
    let settings = gen_settings(plugin);
    let connect = gen_connect(&plugin.id);
    let outgoing = gen_outgoing(plugin);
    let incoming = gen_incoming(plugin);
    let tokens = quote! {
        use ::touchportal_sdk::protocol;

        #settings

        #connect

        #outgoing

        #incoming
    };
    eprintln!("{tokens}");
    let ast: syn::File = syn::parse2(tokens).unwrap();
    prettyplease::unparse(&ast)
}

impl crate::Setting {
    fn choice_enum_name(&self) -> Ident {
        format_ident!("{}SettingOptions", self.name.to_pascal_case())
    }

    fn to_rust_type(&self) -> TokenStream {
        match self.kind {
            SettingType::Text(_) | SettingType::Multiline(_) => quote! { String },
            SettingType::Number(_) => quote! { f64 },
            SettingType::File(_) | SettingType::Folder(_) => {
                quote! { std::path::PathBuf }
            }
            SettingType::Switch(_) => quote! { bool },
            SettingType::Choice(_) => {
                let name = self.choice_enum_name();
                quote! { #name }
            }
        }
    }
}

fn gen_settings(plugin: &PluginDescription) -> TokenStream {
    let mut enums = TokenStream::default();
    for setting in &plugin.settings {
        if let SettingType::Choice(ChoiceSetting { choices, .. }) = &setting.kind {
            let name = setting.choice_enum_name();
            let choice_variants1 = choices
                .iter()
                .map(|c| format_ident!("{}", c.to_pascal_case()));
            let choice_variants2 = choices
                .iter()
                .map(|c| format_ident!("{}", c.to_pascal_case()));
            let choice_variants3 = choices
                .iter()
                .map(|c| format_ident!("{}", c.to_pascal_case()));
            let help = format!("Valid choices for setting [`{}`]", setting.name);
            enums = quote! {
                #enums

                #[doc = #help]
                #[derive(Debug, Clone, Copy, serde::Deserialize)]
                pub enum #name {
                    #(
                        #[serde(rename = #choices)]
                        #choice_variants1
                    ),*
                }

                impl ::std::fmt::Display for #name {
                    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                        write!(f, "{}", match self {
                            #(
                                Self::#choice_variants2 => #choices
                            ),*
                        })
                    }
                }

                impl ::std::str::FromStr for #name {
                    type Err = eyre::Report;
                    fn from_str(s: &str) -> ::eyre::Result<Self> {
                        match s {
                            #(#choices => Ok(Self::#choice_variants3),)*
                            _ => eyre::bail!("'{s}' is not a valid setting value"),
                        }
                    }
                }

                impl protocol::TouchPortalToString for #name {
                    fn stringify(&self) -> String {
                        self.to_string()
                    }
                }
                impl protocol::TouchPortalFromStr for #name {
                    fn destringify(s: &str) -> eyre::Result<Self> {
                        ::std::str::FromStr::from_str(s)
                    }
                }
            };
        }
    }

    let fields_raw = plugin.settings.iter().map(|s| &s.name);
    let fields1 = plugin
        .settings
        .iter()
        .map(|s| format_ident!("{}", s.name.to_snake_case()));
    let fields2 = plugin
        .settings
        .iter()
        .map(|s| format_ident!("{}", s.name.to_snake_case()));
    let doc = plugin.settings.iter().map(|s| {
        s.tooltip.as_ref().map(|tt| {
            let title = tt
                .title
                .as_ref()
                .map(|t| quote! { #[doc = #t] #[doc = ""] });
            let body = &tt.body;
            let url = tt
                .doc_url
                .as_ref()
                .map(|u| quote! { #[doc = ""] #[doc = #u]  });
            quote! {
                #title
                #[doc = #body]
                #url
            }
        })
    });
    let types = plugin.settings.iter().map(|s| s.to_rust_type());
    let mut default_fn_names = Vec::new();
    let mut default_fn_idents = Vec::new();
    let mut default_fn_defs = Vec::new();
    for s in &plugin.settings {
        let sname = &s.name;
        let name = format!("defaults_for_setting_{}", sname.to_snake_case());
        let ident = syn::Ident::new(&name, proc_macro2::Span::call_site());
        let type_ = s.to_rust_type();
        let default = &s.initial;
        default_fn_names.push(name);
        default_fn_idents.push(ident.clone());
        default_fn_defs.push(quote! {
            fn #ident() -> #type_ {
                protocol::TouchPortalFromStr::destringify(#default).expect(concat!("initial value '", #default , "' is valid for setting `", #sname, "`"))
            }
        });
    }

    quote! {
        #enums

        #( #default_fn_defs )*

        #[derive(Debug, Clone, serde::Deserialize)]
        pub struct PluginSettings {
            #(
                #doc
                #[serde(with = "protocol::serde_tp_stringly")]
                #[serde(rename = #fields_raw, default = #default_fn_names)]
                #fields1: #types
            ),*
        }

        #[automatically_derived]
        impl Default for PluginSettings {
            fn default() -> Self {
                Self {
                    #(
                        #fields2: #default_fn_idents()
                    ),*
                }
            }
        }

        impl PluginSettings {
            pub fn from_info_settings(info: Vec<::std::collections::HashMap<String, ::serde_json::Value>>) -> ::eyre::Result<Self> {
                use ::eyre::Context as _;
                let value = info.into_iter().flatten().collect();
                serde_json::from_value(value).context("parse settings")
            }

            pub fn from_settings_message(settings: protocol::SettingsMessage) -> ::eyre::Result<Self> {
                use ::eyre::Context as _;
                let value = settings.values.into_iter().flatten().collect();
                serde_json::from_value(value).context("parse settings message")
            }
        }
    }
}

fn gen_outgoing(plugin: &PluginDescription) -> TokenStream {
    let mut state_stuff = Vec::new();
    for state in plugin.categories.iter().flat_map(|c| &c.states) {
        let id = &state.id;
        let description = &state.description;
        let state_name = format_ident!("update_{}", state.id.to_snake_case());
        match &state.kind {
            crate::StateType::Choice(choice_state) => {
                let name = format_ident!("ValuesForState{}", state.id.to_pascal_case());
                let choices = choice_state.choices.iter();
                let choice_variants1 = choice_state
                    .choices
                    .iter()
                    .map(|c| format_ident!("{}", c.to_pascal_case()));
                let choice_variants2 = choice_state
                    .choices
                    .iter()
                    .map(|c| format_ident!("{}", c.to_pascal_case()));
                let help = format!("Valid choices for [`{state_name}`]");
                state_stuff.push(quote! {
                    #[doc = #help]
                    #[derive(Debug, Clone, Copy)]
                    pub enum #name {
                        #(
                            #choice_variants1
                        ),*
                    }

                    impl ::std::fmt::Display for #name {
                        fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                            write!(f, "{}", match self {
                                #(
                                    Self::#choice_variants2 => #choices
                                ),*
                            })
                        }
                    }

                    impl TouchPortalHandle {
                        #[doc = #description]
                        pub async fn #state_name(&mut self, value: #name) {
                            let _ = self.0.send(protocol::TouchPortalCommand::StateUpdate(
                                protocol::UpdateStateCommand::builder()
                                  .state_id(#id)
                                  .value(value.to_string())
                                  .build()
                                  .unwrap()
                            )).await;
                        }
                    }
                });
            }
            crate::StateType::Text(_) => state_stuff.push(quote! {
                impl TouchPortalHandle {
                    #[doc = #description]
                    pub async fn #state_name(&mut self, value: impl Into<String>) {
                        let _ = self.0.send(protocol::TouchPortalCommand::StateUpdate(
                            protocol::UpdateStateCommand::builder()
                              .state_id(#id)
                              .value(value.into())
                              .build()
                              .unwrap()
                        )).await;
                    }
                }
            }),
        }
    }

    let mut already_handled_data_ids = BTreeSet::new();
    let mut action_list_methods = Vec::new();
    for action in plugin.categories.iter().flat_map(|c| &c.actions) {
        match action.implementation {
            ActionImplementation::Static(_) => continue,
            ActionImplementation::Dynamic => {}
        }

        for Data { id, format } in &action.data {
            let DataFormat::Choice(_) = format else {
                continue;
            };
            if !already_handled_data_ids.insert(id) {
                // duplicate data id, but we know it has the same definition, so all is fine
                continue;
            }

            let fn_name = format_ident!("update_choices_in_{}", id);
            let doc = format!("Updates the choice list for the action data field {id}.");
            let specific_fn_name = format_ident!("update_choices_in_specific_{}", id);
            let specific_doc = format!(
                "Updates the choice list for a particular instance of the action data field {id}."
            );
            action_list_methods.push(quote! {
                #[doc = #doc]
                pub async fn #fn_name(&mut self, choices: impl IntoIterator<Item = impl Into<String>>) {
                    let _ = self.0.send(protocol::TouchPortalCommand::ChoiceUpdate(
                        protocol::ChoiceUpdateCommand::builder()
                          .id(#id)
                          .choices(choices.into_iter().map(Into::into).collect())
                          .build()
                          .unwrap()
                    )).await;
                }
                #[doc = #specific_doc]
                #[doc = ""]
                #[doc = "Specifically, this will only update the choice list in the given action instance."]
                #[doc = "You will generally get the instance from a call to one of the `on_select` methods."]
                pub async fn #specific_fn_name(&mut self, instance: impl Into<String>, choices: impl IntoIterator<Item = impl Into<String>>) {
                    let _ = self.0.send(protocol::TouchPortalCommand::ChoiceUpdate(
                        protocol::ChoiceUpdateCommand::builder()
                          .id(#id)
                          .choices(choices.into_iter().map(Into::into).collect())
                          .instance_id(instance)
                          .build()
                          .unwrap()
                    )).await;
                }
            });
        }
    }

    let mut event_methods = Vec::new();
    for event in plugin.categories.iter().flat_map(|c| &c.events) {
        let id = &event.id;
        let format = event
            .format
            .replace("$val", "`$val`")
            .replace("$compare", "`$compare`");
        let mut args_signature = Vec::new();
        let mut args_handle = quote! {};
        let mut args_doc = if event.local_states.is_empty() {
            quote! {}
        } else {
            // TODO: https://github.com/rust-lang/rust/issues/57525
            quote! {
                #[doc = ""]
                #[doc = "Arguments:"]
                #[doc = ""]
            }
        };
        for local in &event.local_states {
            let id = &local.id;
            let arg = format_ident!("{}", id.to_snake_case());
            let doc = format!("- `{}`: {}", arg, local.name);
            args_signature.push(quote! { #arg: impl protocol::TouchPortalToString });
            args_handle = quote! {
                #args_handle
                builder.state((String::from(#id), #arg.stringify()));
            };
            args_doc = quote! {
                #args_doc
                #[doc = #doc]
            };
        }
        let (event_name, doc) = if event.format.contains("$val") {
            let doc = quote! {
                #[doc = #format]
                #[doc = ""]
                #[doc = "Since this value contains `$val`, you probably do not want "]
                #[doc = "to trigger it manually as the current value of the associated "]
                #[doc = "state may not match the user's set `$val` (and TouchPortal "]
                #[doc = "won't check against `$val`)."]
            };
            let event_name = format_ident!("force_trigger_{}", event.id.to_snake_case());
            (event_name, doc)
        } else {
            let doc = quote! {
                #[doc = #format]
            };
            let event_name = format_ident!("trigger_{}", event.id.to_snake_case());
            (event_name, doc)
        };
        event_methods.push(quote! {
            #doc
            #args_doc
            pub async fn #event_name(&mut self, #( #args_signature ),*) {
                let mut builder = protocol::TriggerEventCommand::builder();
                #args_handle
                let _ = self.0.send(protocol::TouchPortalCommand::TriggerEvent(
                    builder
                      .event_id(#id)
                      .build()
                      .unwrap()
                )).await;
            }
        });
    }

    let mut setting_methods = Vec::new();
    for setting in &plugin.settings {
        let name = &setting.name;
        let desc = setting.tooltip.as_ref().map(|tt| {
            let title = tt
                .title
                .as_ref()
                .map(|t| quote! { #[doc = #t] #[doc = ""] });
            let body = &tt.body;
            let url = tt
                .doc_url
                .as_ref()
                .map(|u| quote! { #[doc = ""] #[doc = #u]  });
            quote! {
                #title
                #[doc = #body]
                #url
            }
        });
        let arg_type = setting.to_rust_type();
        let setter_name = format_ident!("set_{}", name.to_snake_case());
        setting_methods.push(quote! {
            #desc
            pub async fn #setter_name(&mut self, value: #arg_type) {
                let _ = self.0.send(protocol::TouchPortalCommand::SettingUpdate(
                    protocol::UpdateSettingCommand::builder()
                      .name(#name)
                      .value(value.to_string())
                      .build()
                      .unwrap()
                )).await;
            }
        });
    }

    quote! {
        #[derive(Clone, Debug)]
        pub struct TouchPortalHandle(::tokio::sync::mpsc::Sender<protocol::TouchPortalCommand>);

        impl TouchPortalHandle {
            /// As a plug-in developer you can alert your users within Touch Portal for certain events.
            ///
            /// This system should only be used for important messages that the user has to act on. Examples
            /// are new updates for the plugin or changing settings like credentials. Maybe your user has set
            /// up the plug-in incorrectly which is also a good reason to send a notification to alert them to
            /// the issue and propose a solution.
            ///
            /// <div class="warning">
            ///
            /// **Rules of notifications**
            ///
            /// You are only allowed to send user critical notifications to help them on their way.
            /// Advertisements, donation request and all other non-essential messages are not allowed and may
            /// result in your plug-in be blacklisted from the notification center.
            ///
            /// </div>
            pub async fn notify(&mut self, cmd: protocol::CreateNotificationCommand) {
                let _ = self.0.send(protocol::TouchPortalCommand::CreateNotification(cmd)).await;
            }

            /// Create a state at runtime.
            pub async fn create_state(&mut self, cmd: protocol::CreateStateCommand) {
                let _ = self.0.send(protocol::TouchPortalCommand::CreateState(cmd)).await;
            }

            /// Remove a state at runtime.
            pub async fn remove_state(&mut self, id: impl Into<String>) {
                let _ = self.0.send(protocol::TouchPortalCommand::RemoveState(
                    protocol::RemoveStateCommand::builder()
                        .id(id)
                        .build()
                        .unwrap()
                )).await;
            }

            #( #event_methods )*

            #( #setting_methods )*

            #( #action_list_methods )*
        }

        #( #state_stuff )*
    }
}

impl Data {
    fn choice_enum_name(&self) -> Ident {
        format_ident!("ChoicesFor{}", self.id.to_pascal_case())
    }
}

fn gen_incoming(plugin: &PluginDescription) -> TokenStream {
    let mut action_data_choices = quote! {};
    let mut action_ids = Vec::new();
    let mut action_signatures = Vec::new();
    let mut action_arms = Vec::new();
    let mut handled_data_choice_ids = BTreeSet::new();
    for action in plugin.categories.iter().flat_map(|c| &c.actions) {
        match action.implementation {
            ActionImplementation::Static(_) => continue,
            ActionImplementation::Dynamic => {}
        }

        let id = &action.id;
        let name = format_ident!("on_{}", action.id.to_snake_case());
        action_ids.push(id);
        let mut args = IndexMap::new();
        for data @ Data { id, format } in &action.data {
            let arg_type = match format {
                DataFormat::Text(_) => quote! { String },
                DataFormat::Number(_) => quote! { f64 },
                DataFormat::Switch(_) => quote! { bool },
                DataFormat::Choice(choice_data) => {
                    let name = data.choice_enum_name();
                    if handled_data_choice_ids.insert(id) {
                        let choices = &choice_data.value_choices;
                        let as_variant = |c: &str| {
                            if c.is_empty() {
                                format_ident!("Empty")
                            } else {
                                format_ident!("{}", c.to_pascal_case())
                            }
                        };
                        let choice_variants1 = choices.iter().map(|c| as_variant(c));
                        let choice_variants2 = choices.iter().map(|c| as_variant(c));
                        let choice_variants3 = choices.iter().map(|c| as_variant(c));
                        action_data_choices = quote! {
                            #action_data_choices

                            #[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
                            #[allow(non_camel_case_types)]
                            #[allow(non_snake_case)]
                            pub enum #name {
                                #(
                                    #[serde(rename = #choices)]
                                    #choice_variants1,
                                )*

                                /// Used when a choice value has been dynamically created at runtime
                                /// using `update_choices_in*`.
                                #[serde(untagged)]
                                Dynamic(String)
                            }

                            impl ::std::fmt::Display for #name {
                                fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                                    write!(f, "{}", match self {
                                        #(
                                            Self::#choice_variants2 => #choices,
                                        )*
                                        Self::Dynamic(other) => other,
                                    })
                                }
                            }

                            impl ::std::str::FromStr for #name {
                                type Err = eyre::Report;
                                fn from_str(s: &str) -> ::eyre::Result<Self> {
                                    match s {
                                        #(#choices => Ok(Self::#choice_variants3),)*
                                        _ => Ok(Self::Dynamic(s.to_string())),
                                    }
                                }
                            }

                            impl protocol::TouchPortalToString for #name {
                                fn stringify(&self) -> String {
                                    self.to_string()
                                }
                            }
                            impl protocol::TouchPortalFromStr for #name {
                                fn destringify(s: &str) -> eyre::Result<Self> {
                                    ::std::str::FromStr::from_str(s)
                                }
                            }
                        };
                    }
                    quote! { #name }
                }
                DataFormat::File(_) | DataFormat::Folder(_) => quote! { ::std::path::PathBuf },
                DataFormat::Color(_) => quote! { ::touchportal_sdk::reexports::HexColor },
                DataFormat::LowerBound(_) | DataFormat::UpperBound(_) => quote! { i64 },
            };
            args.insert(format_ident!("{}", id.to_snake_case()), arg_type);
        }
        let arg_names = args.keys();
        let arg_types = args.values();
        action_signatures.push(quote! {
            async fn #name(
                &mut self,
                mode: protocol::ActionInteractionMode,
                #( #arg_names: #arg_types ),*
            ) -> eyre::Result<()>;
        });
        let arg_names1 = args.keys();
        let arg_names2 = args.keys();
        let arg_names3 = args.keys();
        let arg_names4 = args.keys();
        let arg_names5 = args.keys();
        let arg_types = args.values();
        action_arms.push(quote! {{
            #[allow(unused_mut)]
            let mut args: ::std::collections::HashMap<_, _> = action.data.into_iter().map(|idv| (idv.id, idv.value)).collect();
            ::tracing::trace!(?args, concat!("action ", #id, " called"));
            #(
                let #arg_names3: #arg_types = {
                    let arg = args
                      .remove(stringify!(#arg_names1))
                      .ok_or_else(|| eyre::eyre!(concat!("action ", #id, " called without argument ", stringify!(#arg_names2))))?;
                    protocol::TouchPortalFromStr::destringify(&arg)
                      .context(concat!("action ", #id, " called with incorrectly typed argument ", stringify!(#arg_names4)))?
                };
            )*
            self.#name(
                interaction_mode,
                #( #arg_names5 ),*
            ).await.context(concat!("handle ", #id, " action"))?
        }});
    }

    let mut list_ids = Vec::new();
    let mut list_id_for_actions = Vec::new();
    let mut list_signatures = Vec::new();
    let mut list_arms = Vec::new();
    for action in plugin.categories.iter().flat_map(|c| &c.actions) {
        match action.implementation {
            ActionImplementation::Static(_) => continue,
            ActionImplementation::Dynamic => {}
        }

        for data @ Data { id, format } in &action.data {
            let DataFormat::Choice(_) = format else {
                continue;
            };

            list_ids.push(id);
            list_id_for_actions.push(&action.id);
            let enum_type = data.choice_enum_name();

            let name = format_ident!(
                "on_select_{}_in_{}",
                id.to_snake_case(),
                action.id.to_snake_case()
            );
            list_signatures.push(quote! {
                async fn #name(
                    &mut self,
                    instance: String,
                    selected: #enum_type,
                ) -> eyre::Result<()>;
            });
            list_arms.push(quote! {{
                let value: #enum_type = protocol::TouchPortalFromStr::destringify(&change.value)
                      .with_context(|| format!(concat!("list change for choice ", #id, " called with incorrectly typed select value '{}'"), change.value))?;
                self.#name(change.instance_id, value).await.context(concat!("handle ", #id, " list change"))?;
            }});
        }
    }
    let other_arms = (!list_ids.is_empty()).then(|| {
        let unique_list_ids: BTreeSet<_> = list_ids.iter().collect();
        let unique_list_actions: BTreeSet<_> = list_id_for_actions.iter().collect();
        quote! {
            (#(#unique_list_ids)|*, aid) => eyre::bail!("list with known id '{}' changed, but with unexpected action id '{aid}'", change.list_id),
            (lid, #(#unique_list_actions)|*) => eyre::bail!("unknown list with id '{lid}' changed in known action '{}'", change.action_id),
        }
    });

    quote! {
        #[diagnostic::on_unimplemented(
            message = "`{Self}` must implement `PluginCallbacks` to receive updates from TouchPortal ",
            label = "the trait `PluginCallbacks` is not implemented for `{Self}`",
            note = "Add `impl PluginCallbacks for {Self} {{}}` and let your IDE or the compiler guide you.",
            note = "This trait has methods for all possible \"incoming\" messages based on your plugin description in `build.rs`.",
        )]
        trait PluginCallbacks {
            #( #action_signatures )*
            #( #list_signatures )*
            async fn on_broadcast(&mut self, event: protocol::BroadcastEvent) -> eyre::Result<()> {
                tracing::debug!(?event, "on_broadcast noop");
                Ok(())
            }
            async fn on_close(&mut self, eof: bool) -> eyre::Result<()> {
                tracing::debug!(?eof, "on_close noop");
                Ok(())
            }
            async fn on_notification_clicked(&mut self, event: protocol::NotificationClickedMessage) -> eyre::Result<()> {
                tracing::debug!(?event, "on_notification_clicked noop");
                Ok(())
            }
            /// Called when the user modifies and saves plugin settings in TouchPortal.
            ///
            /// This method is triggered when TouchPortal sends a settings update message containing
            /// the complete current state of all plugin settings. Use this to synchronize your
            /// plugin's internal configuration with user-modified settings.
            ///
            /// You will generally want to take the `settings` argument using exhaustive
            /// struct-destructure syntax (i.e., `PluginSettings { field1, field2: _ }`) so that the
            /// compiler will remind you to update the method if new settings are added.
            ///
            /// Remember that this will also be triggered when read-only settings are updated, even
            /// though they're updated _by the plugin_. You'll probably want to ignore updates to
            /// such settings.
            ///
            /// # Arguments
            /// * `settings` - The complete current plugin settings parsed into the generated `PluginSettings` struct
            ///
            /// # Example
            /// ```rust,ignore
            /// async fn on_settings_changed(&mut self, settings: PluginSettings { api_key, another_setting: _ }) -> eyre::Result<()> {
            ///     tracing::info!(?settings, "settings updated by user");
            ///
            ///     // Update your plugin's internal state based on new settings
            ///     self.api_client.update_credentials(&api_key)?;
            ///     self.reconnect_if_needed().await?;
            ///
            ///     Ok(())
            /// }
            /// ```
            async fn on_settings_changed(&mut self, settings: PluginSettings) -> eyre::Result<()>;
        }

        #action_data_choices

        #[allow(private_bounds)]
        impl Plugin where Self: PluginCallbacks {
            async fn handle_incoming(&mut self, msg: protocol::TouchPortalOutput) -> eyre::Result<bool> {
                use protocol::TouchPortalOutput;
                use ::eyre::Context as _;

                match msg {
                    TouchPortalOutput::Info(_) => eyre::bail!("got unexpected late info"),
                    TouchPortalOutput::Action(_)
                        | TouchPortalOutput::Up(_)
                        | TouchPortalOutput::Down(_)
                        => {
                        #[allow(unused_variables)]
                        let (interaction_mode, action) = match msg {
                            TouchPortalOutput::Action(action) => (protocol::ActionInteractionMode::Execute, action),
                            TouchPortalOutput::Down(action) => (protocol::ActionInteractionMode::HoldDown, action),
                            TouchPortalOutput::Up(action) => (protocol::ActionInteractionMode::HoldUp, action),
                            _ => unreachable!("we would not have entered this outer match arm otherwise"),
                        };

                        #[allow(clippy::match_single_binding)]
                        match &*action.action_id {
                            #(
                                #action_ids => #action_arms
                            ),*
                            id => eyre::bail!("action executed with unknown action id {id}"),
                        }
                    },
                    TouchPortalOutput::ConnectorChange(change) => {
                        ::tracing::error!(?change, "connector changes are not yet implemented");
                    },
                    TouchPortalOutput::ShortConnectorIdNotification(assoc) => {
                        ::tracing::error!(?assoc, "short connector id support are not yet implemented");
                    }
                    TouchPortalOutput::ListChange(change) => {
                        #[allow(clippy::match_single_binding)]
                        match (&*change.list_id, &*change.action_id) {
                            #(
                                (#list_ids, #list_id_for_actions) => #list_arms,
                            )*
                            #other_arms
                            (lid, aid) => eyre::bail!("unknown list '{lid}' in unknown action '{aid}' changed"),
                        }
                    }
                    TouchPortalOutput::ClosePlugin(_) => {
                        self.on_close(false).await.context("handle graceful plugin close")?;
                        return Ok(true); // Signal to exit the main loop
                    },
                    TouchPortalOutput::Broadcast(event) => {
                        self.on_broadcast(event).await.context("handle broadcast event")?;
                    },
                    TouchPortalOutput::NotificationOptionClicked(event) => {
                        self.on_notification_clicked(event).await.context("handle notification click")?;
                    }
                    TouchPortalOutput::Settings(settings_msg) => {
                        let settings = PluginSettings::from_settings_message(settings_msg).context("parse settings from message")?;
                        self.on_settings_changed(settings).await.context("handle settings change")?;
                    }
                    _ => unimplemented!("codegen macro must be updated to handle {msg:?}"),
                }

                Ok(false) // Continue running
            }
        }
    }
}

fn gen_connect(plugin_id: &str) -> TokenStream {
    quote! {
        #[allow(private_bounds)]
        impl Plugin where Self: PluginCallbacks {
            /// Run a dynamic plugin against TouchPortal running at the given `addr`.
            ///
            /// `constructor` is used to construct the [`Plugin`] type. This is generic to allow
            /// callers to make last-minute adjustments to `Plugin` before we start using it for
            /// real. Handy for injecting references to things like mock expectations.
            pub async fn run_dynamic_with<C>(addr: impl tokio::net::ToSocketAddrs, constructor: C) -> eyre::Result<()>
            where C: AsyncFnOnce(
                PluginSettings,
                TouchPortalHandle,
                InfoMessage,
            ) -> eyre::Result<Self>
            {
                use protocol::*;
                use ::eyre::Context as _;
                use ::tokio::io::{AsyncBufReadExt, AsyncWriteExt};

                ::tracing::info!("connect to TouchPortal");
                let mut connection = tokio::net::TcpStream::connect(addr)
                    .await
                    .context("connect to TouchPortal host")?;
                ::tracing::info!("connected to TouchPortal");

                let (read, write) = connection.split();
                let mut writer = tokio::io::BufWriter::new(write);
                let mut reader = tokio::io::BufReader::new(read);

                ::tracing::debug!("connected to TouchPortal");
                let mut json = serde_json::to_string(
                    &TouchPortalCommand::Pair(PairCommand {
                        id: #plugin_id.to_string(),
                    }),
                )
                .context("write out pair command")?;
                ::tracing::trace!(?json, "send");
                json.push('\n');
                writer.write_all(json.as_bytes()).await.context("send trailing newline")?;
                writer.flush().await.context("flush pair command")?;

                ::tracing::debug!("await info response");
                let mut line = String::new();
                let n = reader
                    .read_line(&mut line)
                    .await
                    .context("retrieve plugin info from server")?;
                if n == 0 {
                    eyre::bail!("TouchPortal closed connection on pair");
                }
                let json = serde_json::from_str(&line)
                    .context("parse plugin info from server")?;

                ::tracing::trace!(?json, "recv");
                let output: TouchPortalOutput =
                    serde_json::from_value(json).context("parse as TouchPortalOutput")?;

                let TouchPortalOutput::Info(mut info) = output else {
                    eyre::bail!("did not receive info in response to pair, got {output:?}");
                };

                let settings = if info.settings.is_empty() {
                    ::tracing::debug!("use default settings");
                    PluginSettings::default()
                } else {
                    ::tracing::debug!("parse customized settings");
                    PluginSettings::from_info_settings(std::mem::take(&mut info.settings))
                        .context("parse settings from info")?
                };

                ::tracing::debug!("construct Plugin proper");
                let (send_outgoing, mut outgoing) = tokio::sync::mpsc::channel(32);
                let mut plugin = constructor(settings, TouchPortalHandle(send_outgoing), info)
                    .await
                    .context("run Plugin constructor")?;

                // Set up re-use buffers
                let mut line = String::new();
                let mut out_buf = Vec::new();

                loop {
                    tokio::select! {
                        n = reader.read_line(&mut line) => {
                            let n = n.context("read incoming message from TouchPortal")?;
                            if n == 0 {
                                ::tracing::warn!("incoming channel from TouchPortal terminated");
                                plugin.on_close(true).await.context("handle server-side EOF")?;
                                break;
                            }
                            let json: serde_json::Value = serde_json::from_str(&line)
                                .context("parse JSON from TouchPortal")?;
                            let kind = json["type"].to_string();
                            ::tracing::trace!(?json, "recv");
                            let msg: TouchPortalOutput =
                                serde_json::from_value(json)
                                .context("parse as TouchPortalOutput")?;
                            let should_exit = plugin
                                .handle_incoming(msg)
                                .await
                                .with_context(|| format!("respond to {kind}"))?;
                            if should_exit {
                                ::tracing::info!("plugin received close signal, exiting gracefully");
                                break;
                            }
                            line.clear();
                        }
                        cmd = outgoing.recv(), if !outgoing.is_closed() => {
                            let Some(cmd) = cmd else {
                                // Plugin shutting down?
                                ::tracing::warn!("outgoing channel to TouchPortal terminated");
                                break;
                            };

                            serde_json::to_writer(&mut out_buf, &cmd)
                              .context("serialize outgoing command")?;
                            let json = std::str::from_utf8(&out_buf).expect("JSON is valid UTF-8");
                            ::tracing::trace!(?json, "send");
                            out_buf.push(b'\n');
                            writer
                                .write_all(&out_buf)
                                .await
                                .context("send outgoing command to TouchPortal")?;
                            writer
                                .flush()
                                .await
                                .context("flush outgoing command")?;
                            out_buf.clear();
                        }
                    };
                }

                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{reexport::HexColor, *};
    use insta::assert_snapshot;

    /// Helper function to create a minimal plugin for testing
    fn minimal_plugin() -> PluginDescription {
        PluginDescription::builder()
            .api(ApiVersion::V4_3)
            .version(1)
            .name("Test Plugin")
            .id("com.test.plugin")
            .configuration(
                PluginConfiguration::builder()
                    .color_dark(HexColor::from_u24(0x282828))
                    .color_light(HexColor::from_u24(0xff0000))
                    .parent_category(PluginCategory::Misc)
                    .build()
                    .unwrap(),
            )
            .plugin_start_cmd("test-plugin.exe")
            .build()
            .unwrap()
    }

    /// Helper function to create a plugin with various settings for testing
    fn plugin_with_settings() -> PluginDescription {
        PluginDescription::builder()
            .api(ApiVersion::V4_3)
            .version(1)
            .name("Settings Test Plugin")
            .id("com.test.settings")
            .configuration(
                PluginConfiguration::builder()
                    .color_dark(HexColor::from_u24(0x123456))
                    .color_light(HexColor::from_u24(0xffffff))
                    .parent_category(PluginCategory::Misc)
                    .build()
                    .unwrap(),
            )
            .setting(
                Setting::builder()
                    .name("text_setting")
                    .initial("default_text")
                    .kind(SettingType::Text(
                        TextSetting::builder()
                            .max_length(100)
                            .is_password(false)
                            .read_only(false)
                            .build()
                            .unwrap(),
                    ))
                    .build()
                    .unwrap(),
            )
            .setting(
                Setting::builder()
                    .name("choice_setting")
                    .initial("Option A")
                    .kind(SettingType::Choice(
                        ChoiceSetting::builder()
                            .choice("Option A")
                            .choice("Option B")
                            .choice("Option C")
                            .build()
                            .unwrap(),
                    ))
                    .build()
                    .unwrap(),
            )
            .setting(
                Setting::builder()
                    .name("number_setting")
                    .initial("42")
                    .kind(SettingType::Number(
                        NumberSetting::builder()
                            .min_value(0.0)
                            .max_value(100.0)
                            .build()
                            .unwrap(),
                    ))
                    .build()
                    .unwrap(),
            )
            .setting(
                Setting::builder()
                    .name("switch_setting")
                    .initial("On")
                    .kind(SettingType::Switch(
                        SwitchSetting::builder().build().unwrap(),
                    ))
                    .build()
                    .unwrap(),
            )
            .plugin_start_cmd("settings-test.exe")
            .build()
            .unwrap()
    }

    /// Helper function to create a plugin with actions for testing
    fn plugin_with_actions() -> PluginDescription {
        PluginDescription::builder()
            .api(ApiVersion::V4_3)
            .version(1)
            .name("Actions Test Plugin")
            .id("com.test.actions")
            .configuration(
                PluginConfiguration::builder()
                    .color_dark(HexColor::from_u24(0x000000))
                    .color_light(HexColor::from_u24(0xffffff))
                    .parent_category(PluginCategory::Misc)
                    .build()
                    .unwrap(),
            )
            .category(
                Category::builder()
                    .id("test_category")
                    .name("Test Actions")
                    .action(
                        Action::builder()
                            .id("test_action")
                            .name("Test Action")
                            .implementation(ActionImplementation::Dynamic)
                            .datum(
                                Data::builder()
                                    .id("text_data")
                                    .format(DataFormat::Text(TextData::builder().build().unwrap()))
                                    .build()
                                    .unwrap(),
                            )
                            .datum(
                                Data::builder()
                                    .id("choice_data")
                                    .format(DataFormat::Choice(
                                        ChoiceData::builder()
                                            .initial("First")
                                            .choice("First")
                                            .choice("Second")
                                            .build()
                                            .unwrap(),
                                    ))
                                    .build()
                                    .unwrap(),
                            )
                            .lines(
                                Lines::builder()
                                    .action(
                                        LingualLine::builder()
                                            .datum(
                                                Line::builder()
                                                    .line_format(
                                                        "Test {$text_data$} with {$choice_data$}",
                                                    )
                                                    .build()
                                                    .unwrap(),
                                            )
                                            .build()
                                            .unwrap(),
                                    )
                                    .build()
                                    .unwrap(),
                            )
                            .build()
                            .unwrap(),
                    )
                    .build()
                    .unwrap(),
            )
            .plugin_start_cmd("actions-test.exe")
            .build()
            .unwrap()
    }

    /// Helper function to create a plugin with events and states for testing
    fn plugin_with_events_and_states() -> PluginDescription {
        PluginDescription::builder()
            .api(ApiVersion::V4_3)
            .version(1)
            .name("Events Test Plugin")
            .id("com.test.events")
            .configuration(
                PluginConfiguration::builder()
                    .color_dark(HexColor::from_u24(0x333333))
                    .color_light(HexColor::from_u24(0xcccccc))
                    .parent_category(PluginCategory::Misc)
                    .build()
                    .unwrap(),
            )
            .category(
                Category::builder()
                    .id("test_events_category")
                    .name("Test Events")
                    .event(
                        Event::builder()
                            .id("test_event")
                            .name("Test Event")
                            .format("When test value $compare $val")
                            .value(EventValueType::Text(
                                EventTextConfiguration::builder().build().unwrap(),
                            ))
                            .value_state_id("test_state")
                            .build()
                            .unwrap(),
                    )
                    .state(
                        State::builder()
                            .id("test_state")
                            .description("Test state for events")
                            .initial("default")
                            .kind(StateType::Text(TextState::builder().build().unwrap()))
                            .build()
                            .unwrap(),
                    )
                    .state(
                        State::builder()
                            .id("choice_state")
                            .description("Test choice state")
                            .initial("Red")
                            .kind(StateType::Choice(
                                ChoiceState::builder()
                                    .choice("Red")
                                    .choice("Green")
                                    .choice("Blue")
                                    .build()
                                    .unwrap(),
                            ))
                            .build()
                            .unwrap(),
                    )
                    .build()
                    .unwrap(),
            )
            .plugin_start_cmd("events-test.exe")
            .build()
            .unwrap()
    }

    /// Verifies that generated code is syntactically valid Rust
    fn assert_valid_rust_syntax(code: &str) {
        syn::parse_str::<syn::File>(code).unwrap_or_else(|e| {
            panic!("Generated code has invalid Rust syntax: {e}\n\nCode:\n{code}")
        });
    }

    #[test]
    fn generate_minimal_plugin() {
        let plugin = minimal_plugin();
        let generated = generate(&plugin);

        assert_valid_rust_syntax(&generated);
        assert_snapshot!(generated);
    }

    #[test]
    fn generate_plugin_with_settings() {
        let plugin = plugin_with_settings();
        let generated = generate(&plugin);

        assert_valid_rust_syntax(&generated);
        assert_snapshot!(generated);
    }

    #[test]
    fn generate_plugin_with_actions() {
        let plugin = plugin_with_actions();
        let generated = generate(&plugin);

        assert_valid_rust_syntax(&generated);
        assert_snapshot!(generated);
    }

    #[test]
    fn generate_plugin_with_events_and_states() {
        let plugin = plugin_with_events_and_states();
        let generated = generate(&plugin);

        assert_valid_rust_syntax(&generated);
        assert_snapshot!(generated);
    }

    #[test]
    fn generate_complex_plugin() {
        let plugin = PluginDescription::builder()
            .api(ApiVersion::V4_3)
            .version(2)
            .name("Complex Test Plugin")
            .id("com.test.complex")
            .configuration(
                PluginConfiguration::builder()
                    .color_dark(HexColor::from_u24(0x123456))
                    .color_light(HexColor::from_u24(0xabcdef))
                    .parent_category(PluginCategory::Misc)
                    .build()
                    .unwrap(),
            )
            .setting(
                Setting::builder()
                    .name("complex_choice")
                    .initial("Default")
                    .kind(SettingType::Choice(
                        ChoiceSetting::builder()
                            .choice("Default")
                            .choice("Advanced")
                            .choice("Expert")
                            .build()
                            .unwrap(),
                    ))
                    .tooltip(
                        Tooltip::builder()
                            .title("Complexity Level")
                            .body("Choose your preferred complexity level")
                            .doc_url("https://example.com/docs")
                            .build()
                            .unwrap(),
                    )
                    .build()
                    .unwrap(),
            )
            .category(
                Category::builder()
                    .id("cat1")
                    .name("Category One")
                    .action(
                        Action::builder()
                            .id("action_with_multiple_data")
                            .name("Multi Data Action")
                            .implementation(ActionImplementation::Dynamic)
                            .datum(
                                Data::builder()
                                    .id("text_input")
                                    .format(DataFormat::Text(TextData::builder().build().unwrap()))
                                    .build()
                                    .unwrap(),
                            )
                            .datum(
                                Data::builder()
                                    .id("number_input")
                                    .format(DataFormat::Number(
                                        NumberData::builder().initial(50.0).build().unwrap()
                                    ))
                                    .build()
                                    .unwrap(),
                            )
                            .datum(
                                Data::builder()
                                    .id("switch_input")
                                    .format(DataFormat::Switch(
                                        SwitchData::builder().initial(false).build().unwrap()
                                    ))
                                    .build()
                                    .unwrap(),
                            )
                            .lines(
                                Lines::builder()
                                    .action(
                                        LingualLine::builder()
                                            .datum(
                                                Line::builder()
                                                    .line_format("Process {$text_input$} with number {$number_input$}")
                                                    .build()
                                                    .unwrap(),
                                            )
                                            .datum(
                                                Line::builder()
                                                    .line_format("Switch is {$switch_input$}")
                                                    .build()
                                                    .unwrap(),
                                            )
                                            .build()
                                            .unwrap(),
                                    )
                                    .build()
                                    .unwrap(),
                            )
                            .build()
                            .unwrap(),
                    )
                    .state(
                        State::builder()
                            .id("status_state")
                            .description("Current status")
                            .initial("Ready")
                            .parent_group("Status")
                            .kind(StateType::Text(TextState::builder().build().unwrap()))
                            .build()
                            .unwrap(),
                    )
                    .build()
                    .unwrap(),
            )
            .category(
                Category::builder()
                    .id("cat2")
                    .name("Category Two")
                    .event(
                        Event::builder()
                            .id("value_changed")
                            .name("Value Changed")
                            .format("When value becomes $val")
                            .value(EventValueType::Choice(
                                EventChoiceValue::builder()
                                    .choice("Low")
                                    .choice("Medium")
                                    .choice("High")
                                    .build()
                                    .unwrap(),
                            ))
                            .value_state_id("value_state")
                            .local_state(
                                LocalState::builder()
                                    .id("timestamp")
                                    .name("Change Timestamp")
                                    .build()
                                    .unwrap(),
                            )
                            .build()
                            .unwrap(),
                    )
                    .state(
                        State::builder()
                            .id("value_state")
                            .description("Current value level")
                            .initial("Medium")
                            .kind(StateType::Choice(
                                ChoiceState::builder()
                                    .choice("Low")
                                    .choice("Medium")
                                    .choice("High")
                                    .build()
                                    .unwrap(),
                            ))
                            .build()
                            .unwrap(),
                    )
                    .build()
                    .unwrap(),
            )
            .plugin_start_cmd("complex-test.exe")
            .build()
            .unwrap();

        let generated = generate(&plugin);

        assert_valid_rust_syntax(&generated);
        assert_snapshot!(generated);
    }

    #[test]
    fn generate_empty_plugin() {
        let plugin = PluginDescription::builder()
            .api(ApiVersion::V4_3)
            .version(1)
            .name("Empty Plugin")
            .id("com.test.empty")
            .configuration(
                PluginConfiguration::builder()
                    .color_dark(HexColor::from_u24(0x000000))
                    .color_light(HexColor::from_u24(0xffffff))
                    .parent_category(PluginCategory::Misc)
                    .build()
                    .unwrap(),
            )
            .plugin_start_cmd("empty.exe")
            .build()
            .unwrap();

        let generated = generate(&plugin);

        assert_valid_rust_syntax(&generated);
        assert_snapshot!(generated);
    }

    #[test]
    fn test_gen_settings_individual() {
        let plugin = plugin_with_settings();
        let settings_code = gen_settings(&plugin).to_string();

        assert_valid_rust_syntax(&settings_code);
        assert_snapshot!(settings_code);
    }

    #[test]
    fn test_gen_outgoing_individual() {
        let plugin = plugin_with_events_and_states();
        let outgoing_code = gen_outgoing(&plugin).to_string();

        assert_valid_rust_syntax(&outgoing_code);
        assert_snapshot!(outgoing_code);
    }

    #[test]
    fn test_gen_incoming_individual() {
        let plugin = plugin_with_actions();
        let incoming_code = gen_incoming(&plugin).to_string();

        assert_valid_rust_syntax(&incoming_code);
        assert_snapshot!(incoming_code);
    }

    #[test]
    fn test_gen_connect_individual() {
        let connect_code = gen_connect("com.test.connect").to_string();

        assert_valid_rust_syntax(&connect_code);
        assert_snapshot!(connect_code);
    }

    #[test]
    fn test_choice_enum_name_generation() {
        use crate::{ChoiceSetting, SettingType};
        use insta::assert_snapshot;

        let setting = crate::Setting {
            name: "test_setting".to_string(),
            initial: "option1".to_string(),
            kind: SettingType::Choice(
                ChoiceSetting::builder()
                    .choice("option1")
                    .choice("option2")
                    .build()
                    .unwrap(),
            ),
            tooltip: None,
        };

        let enum_name = setting.choice_enum_name();
        assert_snapshot!(enum_name, @"TestSettingSettingOptions");

        // Test with complex naming patterns including ACRONYMS and numbers
        let setting2 = crate::Setting {
            name: "my_HTTP_API_v2_setting_name".to_string(),
            initial: "choice1".to_string(),
            kind: SettingType::Choice(ChoiceSetting::builder().choice("choice1").build().unwrap()),
            tooltip: None,
        };

        let enum_name2 = setting2.choice_enum_name();
        assert_snapshot!(enum_name2, @"MyHTTPAPIV2SettingNameSettingOptions");
    }

    #[test]
    fn test_data_choice_enum_name_generation() {
        use crate::{ChoiceData, Data, DataFormat};
        use insta::assert_snapshot;

        let data = Data {
            id: "test_data_field".to_string(),
            format: DataFormat::Choice(
                ChoiceData::builder()
                    .initial("option1")
                    .choice("option1")
                    .choice("option2")
                    .build()
                    .unwrap(),
            ),
        };

        let enum_name = data.choice_enum_name();
        assert_snapshot!(enum_name, @"ChoicesForTestDataField");

        // Test with complex data ID patterns including ACRONYMS and numbers
        let data2 = Data {
            id: "my_JSON_API_v3_data_field_42".to_string(),
            format: DataFormat::Choice(
                ChoiceData::builder()
                    .initial("a")
                    .choice("a")
                    .build()
                    .unwrap(),
            ),
        };

        let enum_name2 = data2.choice_enum_name();
        assert_snapshot!(enum_name2, @"ChoicesForMyJSONAPIV3DataField42");
    }

    #[test]
    fn test_setting_to_rust_type() {
        use crate::{ChoiceSetting, SettingType, SwitchSetting, TextSetting};

        // Test text setting type
        let text_setting = crate::Setting {
            name: "text".to_string(),
            initial: "default".to_string(),
            kind: SettingType::Text(TextSetting::builder().build().unwrap()),
            tooltip: None,
        };

        let rust_type = text_setting.to_rust_type();
        assert_eq!(rust_type.to_string(), "String");

        // Test switch setting type
        let switch_setting = crate::Setting {
            name: "switch".to_string(),
            initial: "Off".to_string(),
            kind: SettingType::Switch(SwitchSetting::builder().build().unwrap()),
            tooltip: None,
        };

        let rust_type = switch_setting.to_rust_type();
        assert_eq!(rust_type.to_string(), "bool");

        // Test choice setting type
        let choice_setting = crate::Setting {
            name: "my_choice".to_string(),
            initial: "option1".to_string(),
            kind: SettingType::Choice(
                ChoiceSetting::builder()
                    .choice("option1")
                    .choice("option2")
                    .build()
                    .unwrap(),
            ),
            tooltip: None,
        };

        let rust_type = choice_setting.to_rust_type();
        assert_eq!(rust_type.to_string(), "MyChoiceSettingOptions");
    }

    #[test]
    fn test_gen_settings_with_various_types() {
        use crate::{
            ApiVersion, ChoiceSetting, PluginConfiguration, PluginDescription, SettingType,
            SwitchSetting, TextSetting,
        };

        let plugin = PluginDescription::builder()
            .api(ApiVersion::V4_3)
            .version(1)
            .name("Settings Test Plugin")
            .id("com.test.settings.types")
            .configuration(PluginConfiguration::builder().build().unwrap())
            .plugin_start_cmd("test_plugin.exe")
            .setting(crate::Setting {
                name: "text_setting".to_string(),
                initial: "default_text".to_string(),
                kind: SettingType::Text(TextSetting::builder().build().unwrap()),
                tooltip: None,
            })
            .setting(crate::Setting {
                name: "switch_setting".to_string(),
                initial: "Off".to_string(),
                kind: SettingType::Switch(SwitchSetting::builder().build().unwrap()),
                tooltip: None,
            })
            .setting(crate::Setting {
                name: "choice_setting".to_string(),
                initial: "option_a".to_string(),
                kind: SettingType::Choice(
                    ChoiceSetting::builder()
                        .choice("option_a")
                        .choice("option_b")
                        .choice("option_c")
                        .build()
                        .unwrap(),
                ),
                tooltip: None,
            })
            .build()
            .unwrap();

        let settings_code = gen_settings(&plugin);
        let settings_str = settings_code.to_string();

        // Verify it compiles to valid Rust
        assert_valid_rust_syntax(&settings_str);

        // Format the code for better snapshot readability
        let formatted = prettyplease::unparse(&syn::parse_file(&settings_str).unwrap());

        // Snapshot the generated code to catch unintended changes
        assert_snapshot!(formatted);
    }
}
