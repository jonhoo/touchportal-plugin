//! Mock TouchPortal server for testing plugins.
//!
//! This module provides a mock TouchPortal server that can be used to test plugins
//! without requiring a real TouchPortal instance. The mock server implements the
//! TouchPortal JSON protocol and can simulate actions, events, and other interactions.
//!
//! ## Message Flow
//! - **Incoming to mock server**: Commands from plugin to TouchPortal (e.g., state updates, events)
//!   - These are captured and can be validated with test assertions
//! - **Outgoing from mock server**: Messages from TouchPortal to plugin (e.g., actions, broadcasts)
//!   - These are sent by test scenarios to simulate TouchPortal behavior

use crate::protocol::{self, TouchPortalCommand, TouchPortalOutput};
use eyre::{Context, Result};
use std::collections::{HashMap, VecDeque};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, Mutex};

/// Tracks expected action calls and their arguments for testing.
#[derive(Debug, Clone)]
pub struct MockExpectations {
    expected_calls: Arc<Mutex<HashMap<String, VecDeque<serde_json::Value>>>>,
}

impl MockExpectations {
    /// Create a new expectations tracker.
    pub fn new() -> Self {
        Self {
            expected_calls: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Expect a specific action callback to be called with given arguments.
    pub async fn expect_action_call(
        &self,
        callback_name: impl Into<String>,
        args: serde_json::Value,
    ) {
        let callback_name = callback_name.into();
        let mut expected = self.expected_calls.lock().await;
        expected
            .entry(callback_name)
            .or_insert_with(VecDeque::new)
            .push_back(args);
    }

    /// Record that an action callback was actually called (called from within action callbacks).
    ///
    /// This method immediately checks the call against expectations, it does not wait for
    /// `verify()`.
    pub async fn check_action_call(
        &self,
        callback_name: impl Into<String>,
        args: serde_json::Value,
    ) -> Result<()> {
        let callback_name = callback_name.into();
        tracing::debug!(callback = %callback_name, ?args, "action callback invoked");

        let mut expected = self.expected_calls.lock().await;

        // Check if this callback was expected
        let Some(expected_calls) = expected.get_mut(&callback_name) else {
            eyre::bail!(
                "Unexpected action callback '{}' was called with arguments: {:?}",
                callback_name,
                args
            );
        };

        // Check if we have any more expected calls for this callback
        if expected_calls.is_empty() {
            eyre::bail!(
                "Action callback '{}' was called more times than expected",
                callback_name
            );
        }

        // Check if the arguments match the next expected call
        let Some(expected_args) = expected_calls.front() else {
            // This should never happen since we already checked is_empty() above
            eyre::bail!("Internal error: expected_calls was empty after is_empty() check");
        };
        if expected_args != &args {
            eyre::bail!(
                "Call to '{}' had wrong arguments. Expected: {:?}, Actual: {:?}",
                callback_name,
                expected_args,
                args
            );
        }

        // Remove the matched expectation
        expected_calls.pop_front();

        tracing::info!(callback = %callback_name, "✅ action callback matches expectations");
        Ok(())
    }

    /// Verify that all expected calls were consumed.
    /// Note: Individual call validation happens immediately in check_action_call().
    /// This method only verifies that all expectations were satisfied.
    pub async fn verify(&self) -> Result<()> {
        let expected = self.expected_calls.lock().await;

        // Check that all expected calls were consumed
        for (callback_name, expected_calls) in expected.iter() {
            if !expected_calls.is_empty() {
                eyre::bail!(
                    "Expected {} more calls to '{}' but plugin finished",
                    expected_calls.len(),
                    callback_name
                );
            }
        }

        tracing::info!("✅ All expected action calls were satisfied");
        Ok(())
    }

    /// Clear all expectations.
    pub async fn clear(&self) {
        self.expected_calls.lock().await.clear();
    }
}

impl Default for MockExpectations {
    fn default() -> Self {
        Self::new()
    }
}

/// A mock TouchPortal server for testing plugins.
///
/// This server listens on a TCP port and implements the TouchPortal JSON protocol,
/// allowing plugins to be tested without requiring a real TouchPortal instance.
#[derive(Debug)]
pub struct MockTouchPortalServer {
    listener: TcpListener,
    captured_messages: Arc<Mutex<Vec<TouchPortalCommand>>>,
    action_invocations: Arc<Mutex<Vec<String>>>,
    test_scenarios: Vec<TestScenario>,
    expectations: MockExpectations,
}

/// A test scenario that can be executed by the mock server.
pub struct TestScenario {
    /// Name of the test scenario for logging purposes
    pub name: String,
    /// Messages to send to the plugin in order
    pub messages: Vec<TouchPortalOutput>,
    /// Delay between sending messages
    pub delay: Duration,
    /// Optional assertion function to validate commands sent from plugin to TouchPortal
    pub assertions:
        Option<Box<dyn Fn(&[TouchPortalCommand], &[String]) -> Result<()> + Send + Sync>>,
}

// Manual Debug implementation since Box<dyn Fn> doesn't implement Debug
impl std::fmt::Debug for TestScenario {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TestScenario")
            .field("name", &self.name)
            .field("messages", &self.messages)
            .field("delay", &self.delay)
            .field("assertions", &self.assertions.is_some())
            .finish()
    }
}

// Manual Clone implementation since assertion functions can't be cloned
impl Clone for TestScenario {
    fn clone(&self) -> Self {
        Self {
            name: self.name.clone(),
            messages: self.messages.clone(),
            delay: self.delay,
            assertions: None, // Can't clone function pointers
        }
    }
}

impl MockTouchPortalServer {
    /// Create a new mock TouchPortal server.
    ///
    /// The server will bind to an available port on localhost.
    pub async fn new() -> Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .context("bind mock TouchPortal server")?;

        Ok(Self {
            listener,
            captured_messages: Arc::new(Mutex::new(Vec::new())),
            action_invocations: Arc::new(Mutex::new(Vec::new())),
            test_scenarios: Vec::new(),
            expectations: MockExpectations::new(),
        })
    }

    /// Get the local address the server is bound to.
    ///
    /// Plugins should connect to this address.
    pub fn local_addr(&self) -> Result<SocketAddr> {
        self.listener
            .local_addr()
            .context("get mock TouchPortal server address")
    }

    /// Add a test scenario to be executed when a plugin connects.
    pub fn add_test_scenario(&mut self, scenario: TestScenario) {
        self.test_scenarios.push(scenario);
    }

    /// Get all action IDs that have been invoked during testing.
    pub async fn action_invocations(&self) -> Vec<String> {
        self.action_invocations.lock().await.clone()
    }

    /// Clear all captured action invocations.
    pub async fn clear_action_invocations(&self) {
        self.action_invocations.lock().await.clear();
    }

    /// Get access to the mock expectations for setting up test expectations.
    pub fn expectations(&self) -> &MockExpectations {
        &self.expectations
    }

    /// Get all messages that have been captured from plugin communications.
    pub async fn captured_messages(&self) -> Vec<TouchPortalCommand> {
        self.captured_messages.lock().await.clone()
    }

    /// Clear all captured messages.
    pub async fn clear_captured_messages(&self) {
        self.captured_messages.lock().await.clear();
    }

    /// Get the mock expectations and remove them from the server (for passing to plugin).
    pub fn take_expectations(&mut self) -> MockExpectations {
        std::mem::take(&mut self.expectations)
    }

    /// Run the mock server and execute test scenarios.
    ///
    /// This method will accept connections from plugins and handle the TouchPortal protocol.
    /// It will also execute any configured test scenarios and run assertions.
    pub async fn run_test_scenarios(self) -> Result<()> {
        tracing::info!(
            "mock TouchPortal server listening on {}",
            self.local_addr()?
        );

        // Accept the first connection (assuming single plugin testing)
        let (stream, addr) = self
            .listener
            .accept()
            .await
            .context("accept plugin connection")?;

        tracing::info!("plugin connected from {}", addr);

        self.handle_connection(stream).await
    }

    /// Handle a single plugin connection.
    async fn handle_connection(&self, stream: TcpStream) -> Result<()> {
        let (read_half, write_half) = stream.into_split();
        let mut reader = BufReader::new(read_half);
        let mut writer = BufWriter::new(write_half);

        // Channel for sending messages to the plugin (outgoing from TouchPortal's perspective)
        let (tx, mut rx) = mpsc::channel::<TouchPortalOutput>(32);

        // Spawn task to handle messages going to plugin (outgoing from mock TouchPortal)
        let mut writer_task = {
            tokio::spawn(async move {
                while let Some(message) = rx.recv().await {
                    let json =
                        serde_json::to_string(&message).context("serialize message to plugin")?;

                    tracing::trace!(?json, "mock TouchPortal -> plugin");

                    writer
                        .write_all(json.as_bytes())
                        .await
                        .context("write message to plugin")?;
                    writer
                        .write_all(b"\n")
                        .await
                        .context("write newline to plugin")?;
                    writer.flush().await.context("flush to plugin")?;
                }
                Ok::<(), eyre::Report>(())
            })
        };

        // Spawn task to execute test scenarios (send messages to plugin)
        let mut scenario_task = {
            let tx = tx.clone();
            let mut scenarios = self.test_scenarios.clone();
            let captured_messages = self.captured_messages.clone();
            let action_invocations = self.action_invocations.clone();

            tokio::spawn(async move {
                // Wait a bit for the plugin to initialize
                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

                // Add automatic Pair test at the beginning
                let pair_scenario = TestScenario {
                    name: "Automatic Pair Test".to_string(),
                    messages: Vec::new(),
                    delay: Duration::from_millis(50),
                    assertions: Some(Box::new(|commands, _actions| {
                        let pair_commands = commands
                            .iter()
                            .filter(|cmd| matches!(cmd, TouchPortalCommand::Pair(_)))
                            .count();

                        if pair_commands == 1 {
                            tracing::info!("✅ Plugin successfully paired with mock TouchPortal");
                            Ok(())
                        } else {
                            eyre::bail!("Expected exactly 1 pair command, got {}", pair_commands)
                        }
                    })),
                };
                scenarios.insert(0, pair_scenario);

                // Add automatic ClosePlugin at the end
                let close_scenario = TestScenario {
                    name: "Automatic ClosePlugin".to_string(),
                    messages: vec![TouchPortalOutput::ClosePlugin(
                        protocol::ClosePluginMessage {
                            plugin_id: "mock-plugin".to_string(),
                        },
                    )],
                    delay: Duration::from_millis(100),
                    assertions: None,
                };
                scenarios.push(close_scenario);

                for scenario in scenarios {
                    tracing::info!(scenario.name, "executing test scenario");

                    for message in &scenario.messages {
                        // Track action invocations
                        if let TouchPortalOutput::Action(action_msg) = message {
                            action_invocations
                                .lock()
                                .await
                                .push(action_msg.action_id.clone());
                        }

                        if tx.send(message.clone()).await.is_err() {
                            tracing::warn!("failed to send test message to plugin");
                            break;
                        }

                        if !scenario.delay.is_zero() {
                            tokio::time::sleep(scenario.delay).await;
                        }
                    }

                    // Run assertions if provided
                    if let Some(assertions) = &scenario.assertions {
                        // Wait a moment for any plugin responses to be captured
                        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

                        let messages = captured_messages.lock().await;
                        let actions = action_invocations.lock().await;
                        match assertions(&messages, &actions) {
                            Ok(()) => {
                                tracing::info!(scenario.name, "✅ assertions passed");
                            }
                            Err(e) => {
                                tracing::error!(scenario.name, error = %e, "❌ assertions failed");
                            }
                        }
                    }
                }

                // Give a brief moment for final cleanup
                tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

                tracing::info!("Mock server scenarios completed");
            })
        };

        // Main message handling loop
        let mut line = String::new();
        loop {
            tokio::select! {
                result = reader.read_line(&mut line) => {
                    let n = result.context("read from plugin")?;
                    if n == 0 {
                        tracing::info!("plugin disconnected");
                        break;
                    }

                    let json: serde_json::Value = serde_json::from_str(&line.trim())
                        .context("parse JSON from plugin")?;

                    tracing::trace!(?json, "plugin -> mock TouchPortal");

                    // Parse the command from plugin and store it (incoming to TouchPortal)
                    if let Ok(command) = serde_json::from_value::<TouchPortalCommand>(json.clone()) {
                        self.captured_messages.lock().await.push(command.clone());

                        // Handle specific commands
                        match command {
                            TouchPortalCommand::Pair(_pair_cmd) => {
                                // Respond with mock info message
                                let info = protocol::InfoMessage {
                                    sdk_version: crate::ApiVersion::V4_3,
                                    tp_version_string: "Mock TouchPortal v4.3.0".to_string(),
                                    tp_version_code: 430000,
                                    plugin_version: None,
                                    settings: vec![],
                                    current_page_path_main_device: Some("mock-page.tml".to_string()),
                                    current_page_path_secondary_devices: vec![],
                                };

                                if tx.send(TouchPortalOutput::Info(info)).await.is_err() {
                                    tracing::warn!("failed to send info response to plugin");
                                    break;
                                }
                            }
                            _ => {
                                // For other commands from plugin, just log them
                                tracing::debug!(?command, "received command from plugin");
                            }
                        }
                    }

                    line.clear();
                }
                _ = &mut writer_task, if !writer_task.is_finished() => {
                    tracing::info!("writer task completed");
                    break;
                }
                _ = &mut scenario_task, if !scenario_task.is_finished() => {
                    tracing::info!("scenario task completed");
                    // Continue running to handle plugin responses
                }
            }
        }

        Ok(())
    }
}

impl TestScenario {
    /// Create a new test scenario.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            messages: Vec::new(),
            delay: Duration::from_millis(100),
            assertions: None,
        }
    }

    /// Set the delay between messages.
    pub fn with_delay(mut self, delay: Duration) -> Self {
        self.delay = delay;
        self
    }

    /// Add an assertion function to validate commands sent from plugin to TouchPortal.
    ///
    /// The assertion function receives all TouchPortalCommand messages that the plugin
    /// has sent to the mock TouchPortal server (e.g., StateUpdate, Pair, TriggerEvent)
    /// and a list of action IDs that were invoked during the test.
    pub fn with_assertions<F>(mut self, assertions: F) -> Self
    where
        F: Fn(&[TouchPortalCommand], &[String]) -> Result<()> + Send + Sync + 'static,
    {
        self.assertions = Some(Box::new(assertions));
        self
    }

    /// Add a message to be sent to the plugin.
    pub fn with_message(mut self, message: TouchPortalOutput) -> Self {
        self.messages.push(message);
        self
    }

    /// Add an action message to trigger the plugin.
    pub fn with_action(
        self,
        action_id: impl Into<String>,
        data: Vec<(impl Into<String>, impl Into<String>)>,
    ) -> Self {
        let message = TouchPortalOutput::Action(protocol::ActionMessage {
            plugin_id: "mock-plugin".to_string(),
            action_id: action_id.into(),
            data: data
                .into_iter()
                .map(|(id, value)| protocol::IdValuePair {
                    id: id.into(),
                    value: value.into(),
                })
                .collect(),
        });
        self.with_message(message)
    }

    /// Add a broadcast page change event.
    pub fn with_page_change(
        self,
        page_name: impl Into<String>,
        previous_page_name: Option<impl Into<String>>,
    ) -> Self {
        let message = TouchPortalOutput::Broadcast(protocol::BroadcastEvent::PageChange(
            protocol::BroadcastPageChangeEvent {
                page_name: page_name.into(),
                previous_page_name: previous_page_name.map(|s| s.into()),
                device_ip: Some("127.0.0.1".to_string()),
                device_name: Some("Mock Device".to_string()),
                device_id: Some("mock-device-1".to_string()),
            },
        ));
        self.with_message(message)
    }
}
