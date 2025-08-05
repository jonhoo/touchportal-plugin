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
/// Generates the binding code for your plugin.
///
/// You'll into `$OUT_DIR/touch-portal.rs`.
///
/// You'll usually want to place this into a `.rs` file in `$OUT_DIR` so that you can then include
/// it from your plugin's `main.rs`. That is, in `build.rs`, you'll want:
///
/// ```rust,no_run
/// use touchportal_sdk::{PluginDescription, codegen};
/// fn main() {
///     let plugin = PluginDescription::builder()
///       /* build your plugin manifest here */
///       .build()
///       .unwrap();
///
///     // write out generated code to somewhere your main.rs can include! it from:
///     std::fs::write(
///         format!("{}/entry.rs", std::env::var("OUT_DIR").unwrap()),
///         codegen::build(&plugin),
///     )
///     .unwrap();
///
///     // also write out your serialized plugin manifest (`entry.tp`) to the same place:
///     std::fs::write(
///         format!("{}/entry.tp", std::env::var("OUT_DIR").unwrap()),
///         serde_json::to_vec(&plugin).unwrap(),
///     )
///     .unwrap();
/// }
/// ```
///
/// Then, in `main.rs`, you'll want:
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
/// impl PluginMethods for Plugin {
///     // your IDE/the compiler errors will guide you here
/// }
///
/// #[tokio::main]
/// async fn main() -> eyre::Result<()> {
///     Plugin::run_dynamic("127.0.0.1:12136").await
/// }
/// ```
///
/// To generate your manifest, you can use a bash script like what's below.
/// It ain't pretty, but it works. There are probably nicer ways.
///
/// ```bash
/// #!/bin/bash
///
/// set -euo pipefail
///
/// plugin_name=YouTubeLive
/// crate_binary=touchportal-youtube-live
///
/// build=$(cargo build --release --bin "$crate_binary" -q --message-format=json)
/// exe=$(jq -r "select(.reason == \"compiler-artifact\" and .target.name == \"$crate_binary\").executable" <<<"$build")
/// out_dir="$(dirname "$(jq -r "select(.reason == \"build-script-executed\") | select(.package_id | contains(\"#$crate_binary@\")).out_dir" <<<"$build")")"/out/
/// entry_tp="$out_dir"/entry.tp
///
/// tmp=$(mktemp -d)
/// mkdir "$tmp/$plugin_name"
/// cp "$exe" "$entry_tp" "$tmp/$plugin_name"
/// here=$(pwd)
/// pushd "$tmp"
/// zip -r "$plugin_name.tpp" "$plugin_name"
/// rsync -a "$plugin_name/" ~/.config/TouchPortal/plugins/"$plugin_name"/
/// cp "$plugin_name.tpp" "$here"
/// popd
/// rm -r "$tmp"
/// ```
pub fn build(plugin: &PluginDescription) -> String {
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

    fn string_converter(&self) -> TokenStream {
        quote! { #[serde(with = "protocol::serde_tp_stringly")] }
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
    let converters = plugin.settings.iter().map(|s| s.string_converter());
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
                #converters
                #[serde(rename = #fields_raw, default = #default_fn_names)]
                #fields1: #types
            ),*
        }

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
        let format = event.format.replace("$val", "`$val`");
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
                        let choice_variants1 = choices
                            .iter()
                            .map(|c| format_ident!("{}", c.to_pascal_case()));
                        let choice_variants2 = choices
                            .iter()
                            .map(|c| format_ident!("{}", c.to_pascal_case()));
                        let choice_variants3 = choices
                            .iter()
                            .map(|c| format_ident!("{}", c.to_pascal_case()));
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
        trait PluginMethods {
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
        }

        #action_data_choices

        impl Plugin {
            async fn handle_incoming(&mut self, msg: protocol::TouchPortalOutput) -> eyre::Result<()> {
                use protocol::TouchPortalOutput;
                use ::eyre::Context as _;

                match msg {
                    TouchPortalOutput::Info(_) => eyre::bail!("got unexpected late info"),
                    TouchPortalOutput::Action(_)
                        | TouchPortalOutput::Up(_)
                        | TouchPortalOutput::Down(_)
                        => {
                        let (interaction_mode, action) = match msg {
                            TouchPortalOutput::Action(action) => (protocol::ActionInteractionMode::Execute, action),
                            TouchPortalOutput::Down(action) => (protocol::ActionInteractionMode::HoldDown, action),
                            TouchPortalOutput::Up(action) => (protocol::ActionInteractionMode::HoldUp, action),
                            _ => unreachable!("we would not have entered this outer match arm otherwise"),
                        };

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
                        match (&*change.list_id, &*change.action_id) {
                            #(
                                (#list_ids, #list_id_for_actions) => #list_arms,
                            )*
                            #other_arms
                            (lid, aid) => eyre::bail!("unknown list '{lid}' in unknown action '{aid}' changed"),
                        }
                    }
                    TouchPortalOutput::ClosePlugin(close_plugin_message) => {
                        self.on_close(false).await.context("handle graceful plugin close")?;
                    },
                    TouchPortalOutput::Broadcast(event) => {
                        self.on_broadcast(event).await.context("handle broadcast event")?;
                    },
                    TouchPortalOutput::NotificationOptionClicked(event) => {
                        self.on_notification_clicked(event).await.context("handle notification click")?;
                    }
                    _ => unimplemented!("codegen macro must be updated to handle {msg:?}"),
                }

                Ok(())
            }
        }
    }
}

fn gen_connect(plugin_id: &str) -> TokenStream {
    quote! {
        impl Plugin {
            pub async fn run_dynamic(addr: impl tokio::net::ToSocketAddrs) -> eyre::Result<()> {
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
                let mut plugin = Self::new(settings, TouchPortalHandle(send_outgoing), info)
                    .await
                    .context("Plugin::new")?;

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
                            plugin
                                .handle_incoming(msg)
                                .await
                                .with_context(|| format!("respond to {kind}"))?;
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
