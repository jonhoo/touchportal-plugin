use crate::{
    ActionImplementation, ChoiceSetting, Data, DataFormat, PluginDescription, SettingType,
};

use indexmap::IndexMap;
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::Ident;

/// Generates the binding code for your plugin into `$OUT_DIR/touch-portal.rs`.
pub fn build(plugin: &PluginDescription) -> String {
    // also write out &'static PluginDescription
    // defs probably go to lib, and so does the static (const?) construction of the instance.
    // then, this loads that to make entry.tp _and_ it's used to codegen (how?) action+event bindings.
    // maybe actually there is a crate that has these impls that's then used as a build dep of the main
    // crate?
    let settings = gen_settings(plugin);
    let connect = gen_connect(&plugin.id);
    let outgoing = gen_outgoing();
    let incoming = gen_incoming(&plugin);
    let tokens = quote! {
        use ::touchportal_plugin::protocol;

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
        format_ident!("{}SettingOptions", self.name)
    }

    fn string_converter(&self) -> TokenStream {
        match self.kind {
            SettingType::Number(_) | SettingType::Switch(_) => {
                quote! { #[serde(deserialize_with = "deserialize_with_fromstr")] }
            }
            SettingType::Text(_)
            | SettingType::Multiline(_)
            | SettingType::File(_)
            | SettingType::Folder(_)
            | SettingType::Choice(_) => quote! {},
        }
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
            let choice_variants1 = choices.iter().map(|c| format_ident!("{c}"));
            let choice_variants2 = choices.iter().map(|c| format_ident!("{c}"));
            enums = quote! {
                #enums

                #[derive(Debug, Clone, Copy, serde::Deserialize)]
                #[allow(non_snake_case)]
                pub enum #name {
                    #(
                        #[serde(rename = #choices)]
                        #choice_variants1
                    ),*
                }

                impl ::std::str::FromStr for #name {
                    type Err = ::eyre::Report;
                    fn from_str(s: &str) -> Result<Self, Self::Err> {
                        match s {
                            #(#choices => Ok(Self::#choice_variants2),)*
                            _ => eyre::bail!("'{s}' is not a valid setting value"),
                        }
                    }
                }
            };
        }
    }

    let fields1 = plugin.settings.iter().map(|s| format_ident!("{}", s.name));
    let fields2 = plugin.settings.iter().map(|s| format_ident!("{}", s.name));
    let converters = plugin.settings.iter().map(|s| s.string_converter());
    let types = plugin.settings.iter().map(|s| s.to_rust_type());
    let mut default_fn_names = Vec::new();
    let mut default_fn_idents = Vec::new();
    let mut default_fn_defs = Vec::new();
    for s in &plugin.settings {
        let sname = &s.name;
        let name = format!("defaults_for_setting_{sname}");
        let ident = syn::Ident::new(&name, proc_macro2::Span::call_site());
        let type_ = s.to_rust_type();
        let default = &s.initial;
        default_fn_names.push(name);
        default_fn_idents.push(ident.clone());
        default_fn_defs.push(quote! {
            #[allow(non_snake_case)]
            fn #ident() -> #type_ {
                #default.parse().expect(concat!("initial value '", #default , "' is valid for setting `", #sname, "`"))
            }
        });
    }

    quote! {
        #enums

        #( #default_fn_defs )*

        fn deserialize_with_fromstr<'de, D, T>(deserializer: D) -> Result<T, D::Error>
        where
            D: ::serde::Deserializer<'de>,
            T: ::std::str::FromStr,
            T::Err: ::std::fmt::Display,
        {
            use ::serde::de::Visitor;

            struct V<S>(std::marker::PhantomData<fn() -> S>);

            impl<'de, S> Visitor<'de> for V<S>
            where
                S: ::std::str::FromStr,
                S::Err: ::std::fmt::Display
            {
                type Value = S;

                fn expecting(&self, formatter: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
                    formatter.write_str("a string representing an S")
                }

                fn visit_str<E>(self, v: &str) -> Result<S, E>
                where
                    E: ::serde::de::Error,
                {
                    ::std::str::FromStr::from_str(v).map_err(::serde::de::Error::custom)
                }
            }

            deserializer.deserialize_str(V::<T>(Default::default()))
        }

        #[derive(Debug, Clone, serde::Deserialize)]
        #[allow(non_snake_case)]
        pub struct PluginSettings {
            #(
                #converters
                #[serde(default = #default_fn_names)]
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

fn gen_outgoing() -> TokenStream {
    quote! {
        #[derive(Clone, Debug)]
        pub struct TouchPortalHandle(::tokio::sync::mpsc::Sender<protocol::TouchPortalCommand>);

        impl TouchPortalHandle {
            pub async fn notify(&mut self, cmd: protocol::CreateNotificationCommand) {
                let _ = self.0.send(protocol::TouchPortalCommand::CreateNotification(cmd)).await;
            }
        }
    }
}

fn gen_incoming(plugin: &PluginDescription) -> TokenStream {
    let mut action_data_choices = quote! {};
    let mut action_ids = Vec::new();
    let mut action_signatures = Vec::new();
    let mut action_arms = Vec::new();
    for action in plugin.categories.iter().flat_map(|c| &c.actions) {
        match action.implementation {
            ActionImplementation::Static(_) => continue,
            ActionImplementation::Dynamic => {}
        }

        let id = &action.id;
        let name = format_ident!("on_{}", action.id);
        action_ids.push(id);
        let mut args = IndexMap::new();
        for Data { id, format } in &action.data {
            let path = match format {
                DataFormat::Text(_) => "String",
                DataFormat::Number(_) => "f64",
                DataFormat::Switch(_) => "bool",
                DataFormat::Choice(choice_data) => {
                    let name = format_ident!("ChoicesFor_{id}");
                    let choices = &choice_data.value_choices;
                    let choice_variants = choice_data
                        .value_choices
                        .iter()
                        .map(|c| format_ident!("{c}"));
                    action_data_choices = quote! {
                        #action_data_choices

                        #[derive(Debug, Clone, Copy, serde::Deserialize)]
                        #[allow(non_camel_case_types)]
                        #[allow(non_snake_case)]
                        pub enum #name {
                            #(
                                #[serde(rename = #choices)]
                                #choice_variants
                            ),*
                        }
                    };
                    args.insert(format_ident!("{id}"), name.into());
                    continue;
                }
                DataFormat::File(_) | DataFormat::Folder(_) => "::std::path::PathBuf",
                DataFormat::Color(_) => "::touchportal_plugin::reexports::HexColor",
                DataFormat::LowerBound(_) | DataFormat::UpperBound(_) => "i64",
            };
            let path: syn::Path = syn::parse_str(path).unwrap();
            args.insert(format_ident!("{id}"), path);
        }
        let arg_names = args.keys();
        let arg_types = args.values();
        action_signatures.push(quote! { async fn #name(&mut self, #( #arg_names: #arg_types ),*) -> eyre::Result<()>; });
        let arg_names1 = args.keys();
        let arg_names2 = args.keys();
        let arg_names3 = args.keys();
        let arg_names4 = args.keys();
        let arg_names5 = args.keys();
        let arg_types = args.values();
        action_arms.push(quote! {{
            let mut args: ::std::collections::HashMap<_, _> = action.data.into_iter().flatten().collect();
            #(
                let #arg_names3: #arg_types = {
                    let arg = args
                      .remove(stringify!(#arg_names1))
                      .ok_or_else(|| eyre::eyre!(concat!("action ", #id, " called without argument ", stringify!(#arg_names2))))?;
                    serde_json::from_str(&arg)
                      .context(concat!("action ", #id, " called with incorrectly typed argument ", stringify!(#arg_names4)))?
                };
            )*
            self.#name(#( #arg_names5 ),*).await.context(concat!("handle ", #id, " action"))?
        }});
    }

    quote! {
        trait PluginMethods {
            #( #action_signatures )*
            async fn on_close(&mut self, eof: bool) -> eyre::Result<()>;
        }

        #action_data_choices

        impl Plugin {
            async fn handle_incoming(&mut self, msg: protocol::TouchPortalOutput) -> eyre::Result<()> {
                use protocol::TouchPortalOutput;
                use ::eyre::Context as _;

                match msg {
                    TouchPortalOutput::Info(_) => eyre::bail!("got unexpected late info"),
                    TouchPortalOutput::Action(action) => match &*action.action_id {
                        #(
                            #action_ids => #action_arms
                            id => eyre::bail!("called with unknown action id {id}"),
                        ),*
                    },
                    TouchPortalOutput::Up(hold_message) => todo!(),
                    TouchPortalOutput::Down(hold_message) => todo!(),
                    TouchPortalOutput::ConnectorChange(connector_change_message) => todo!(),
                    TouchPortalOutput::ShortConnectorIdNotification(short_connector_id_message) => todo!(),
                    TouchPortalOutput::ListChange(list_change_message) => todo!(),
                    TouchPortalOutput::ClosePlugin(close_plugin_message) => {
                        self.on_close(false).await.context("handle graceful plugin close")?;
                    }
                    TouchPortalOutput::Broadcast(broadcast_message) => todo!(),
                    TouchPortalOutput::NotificationOptionClicked(notification_clicked_message) => todo!(),
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

                eprintln!("connect to TouchPortal");
                let mut connection = tokio::net::TcpStream::connect(addr)
                    .await
                    .context("connect to TouchPortal host")?;
                eprintln!("connected to TouchPortal");

                let (read, write) = connection.split();
                let mut writer = tokio::io::BufWriter::new(write);
                let mut reader = tokio::io::BufReader::new(read);

                eprintln!("send pair command");
                let mut pair = serde_json::to_string(
                    &TouchPortalCommand::Pair(PairCommand {
                        id: #plugin_id.to_string(),
                    }),
                )
                .context("write out pair command")?;
                pair.push('\n');
                writer.write_all(pair.as_bytes()).await.context("send trailing newline")?;
                writer.flush().await.context("flush pair command")?;

                eprintln!("await info response");
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

                let output: TouchPortalOutput =
                    serde_json::from_value(dbg!(json)).context("parse as TouchPortalOutput")?;

                let TouchPortalOutput::Info(mut info) = output else {
                    eyre::bail!("did not receive info in response to pair, got {output:?}");
                };

                let settings = if info.settings.is_empty() {
                    PluginSettings::default()
                } else {
                    PluginSettings::from_info_settings(std::mem::take(&mut info.settings))
                        .context("parse settings from info")?
                };

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
                                plugin.on_close(true).await.context("handle server-side EOF")?;
                                break;
                            }
                            let json: serde_json::Value = serde_json::from_str(&line)
                                .context("parse JSON from TouchPortal")?;
                            let kind = json["type"].to_string();
                            eprintln!("< {json}");
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
                                break;
                            };

                            serde_json::to_writer(&mut out_buf, &cmd)
                              .context("serialize outgoing command")?;
                            out_buf.push(b'\n');
                            let json = std::str::from_utf8(&out_buf).expect("JSON is valid UTF-8");
                            eprintln!("> {json}");
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
