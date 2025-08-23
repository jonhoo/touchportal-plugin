use crate::plugin::TouchPortalHandle;
use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// Activity level for chat or metrics
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ActivityLevel {
    High,
    Medium,
    Low,
}

impl ActivityLevel {
    pub fn description(&self) -> &'static str {
        match self {
            ActivityLevel::High => "High Activity",
            ActivityLevel::Medium => "Normal",
            ActivityLevel::Low => "Low Activity",
        }
    }
}

/// Tracks chat message activity patterns to determine relative activity levels
#[derive(Debug, Clone)]
pub struct ChatActivityTracker {
    message_timestamps: VecDeque<Instant>,
    session_start: Option<Instant>,
    session_baseline: Option<f64>, // Messages/min when stream started
    rolling_average: f64,          // Recent average (last 15 minutes)
    recent_peak: f64,              // Highest rate in last hour
    last_activity_check: Instant,
    total_messages: u64,
}

impl Default for ChatActivityTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl ChatActivityTracker {
    pub fn new() -> Self {
        Self {
            message_timestamps: VecDeque::new(),
            session_start: None,
            session_baseline: None,
            rolling_average: 0.0,
            recent_peak: 0.0,
            last_activity_check: Instant::now(),
            total_messages: 0,
        }
    }

    pub fn register_message(&mut self) {
        let now = Instant::now();
        self.message_timestamps.push_back(now);
        self.total_messages += 1;

        // Log significant activity changes
        let current_rate = self.messages_per_minute(2); // Immediate activity
        let recent_rate = self.messages_per_minute(10); // Recent baseline
        if current_rate > recent_rate * 3.0 && current_rate > 1.0 {
            tracing::debug!(
                current_rate = %current_rate,
                recent_rate = %recent_rate,
                "chat activity spike detected"
            );
        }

        // Initialize session start on first message
        if self.session_start.is_none() {
            self.session_start = Some(now);
        }

        // Clean old messages (keep last hour)
        let cutoff = now - Duration::from_secs(3600);
        while let Some(&front) = self.message_timestamps.front() {
            if front < cutoff {
                self.message_timestamps.pop_front();
            } else {
                break;
            }
        }

        // Update metrics periodically
        if now.duration_since(self.last_activity_check) > Duration::from_secs(60) {
            self.update_metrics();
            self.last_activity_check = now;
        }
    }

    fn update_metrics(&mut self) {
        let now = Instant::now();

        // Update rolling average (last 15 minutes)
        self.rolling_average = self.messages_per_minute(15);

        // Update recent peak (last hour)
        self.recent_peak = self.recent_peak.max(self.rolling_average);

        // Set session baseline if we have enough data (10 minutes into session)
        if self.session_baseline.is_none()
            && let Some(start) = self.session_start
            && now.duration_since(start) > Duration::from_secs(600)
        {
            // 10 minutes
            self.session_baseline = Some(self.rolling_average);
        }
    }

    fn messages_per_minute(&self, minutes: u64) -> f64 {
        let cutoff = Instant::now() - Duration::from_secs(minutes * 60);
        let count = self
            .message_timestamps
            .iter()
            .filter(|&&timestamp| timestamp >= cutoff)
            .count();
        count as f64 / minutes as f64
    }

    pub fn calculate_activity_level(&self) -> ActivityLevel {
        let current_rate = self.messages_per_minute(5); // Last 5 minutes
        let baseline = self.rolling_average.max(0.1); // Avoid division by zero

        // Multi-window analysis
        let immediate_burst = self.messages_per_minute(2);
        let recent_trend = self.messages_per_minute(10);

        // Check for sudden excitement spike
        if immediate_burst > recent_trend * 3.0 && immediate_burst > 1.0 {
            return ActivityLevel::High;
        }

        // For very quiet streams (< 0.1 msg/min average), any activity is significant
        if baseline < 0.1 {
            return if current_rate > 0.5 {
                ActivityLevel::High
            } else if current_rate > 0.1 {
                ActivityLevel::Medium
            } else {
                ActivityLevel::Low
            };
        }

        // Relative activity level based on established patterns
        let ratio = current_rate / baseline;
        match ratio {
            r if r > 2.0 => ActivityLevel::High,   // 2x+ recent average
            r if r > 1.3 => ActivityLevel::Medium, // 30%+ above average
            r if r < 0.5 => ActivityLevel::Low,    // 50%+ below average
            _ => ActivityLevel::Medium,            // Near average
        }
    }

    pub fn was_inactive_recently(&self) -> bool {
        // Consider inactive if no messages in last 10 minutes but had activity before
        self.messages_per_minute(10) < 0.1 && self.total_messages > 5
    }
}

/// Tracks metrics changes to determine volatility levels
#[derive(Debug, Clone)]
pub struct MetricsVolatilityTracker {
    last_viewers: Option<u64>,
    last_likes: Option<u64>,
    last_views: Option<u64>,
    volatility_history: VecDeque<f64>, // Recent change percentages
    last_update: Instant,
}

impl Default for MetricsVolatilityTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl MetricsVolatilityTracker {
    pub fn new() -> Self {
        Self {
            last_viewers: None,
            last_likes: None,
            last_views: None,
            volatility_history: VecDeque::new(),
            last_update: Instant::now(),
        }
    }

    pub fn update_from_metrics(
        &mut self,
        viewers: Option<u64>,
        likes: Option<u64>,
        views: Option<u64>,
    ) {
        let now = Instant::now();
        let mut total_change = 0.0;
        let mut change_count = 0;

        // Calculate percentage changes
        if let (Some(current), Some(last)) = (viewers, self.last_viewers)
            && last > 0
        {
            let change = (current as f64 - last as f64).abs() / last as f64;
            total_change += change;
            change_count += 1;
        }

        if let (Some(current), Some(last)) = (likes, self.last_likes)
            && last > 0
        {
            let change = (current as f64 - last as f64).abs() / last as f64;
            total_change += change;
            change_count += 1;
        }

        // Views typically grow monotonically, so we look at rate of growth
        if let (Some(current), Some(last)) = (views, self.last_views)
            && last > 0
            && current > last
        {
            let growth_rate = (current as f64 - last as f64) / last as f64;
            total_change += growth_rate;
            change_count += 1;
        }

        // Store average change if we have any measurements
        if change_count > 0 {
            let avg_change = total_change / change_count as f64;
            self.volatility_history.push_back(avg_change);

            // Log significant volatility
            if avg_change > 0.2 {
                // >20% change
                tracing::debug!(
                    avg_change = %avg_change,
                    viewers = ?viewers,
                    likes = ?likes,
                    views = ?views,
                    "high metrics volatility detected"
                );
            }

            // Keep only last 10 measurements
            while self.volatility_history.len() > 10 {
                self.volatility_history.pop_front();
            }
        }

        // Update stored values
        self.last_viewers = viewers;
        self.last_likes = likes;
        self.last_views = views;
        self.last_update = now;
    }

    pub fn calculate_volatility(&self) -> ActivityLevel {
        if self.volatility_history.is_empty() {
            return ActivityLevel::Medium; // Default when no data
        }

        // Average recent volatility
        let avg_volatility: f64 =
            self.volatility_history.iter().sum::<f64>() / self.volatility_history.len() as f64;

        // Check for recent high volatility (last 3 measurements)
        let recent_volatility = if self.volatility_history.len() >= 3 {
            self.volatility_history.iter().rev().take(3).sum::<f64>() / 3.0
        } else {
            avg_volatility
        };

        match recent_volatility {
            v if v > 0.15 => ActivityLevel::High,   // >15% change
            v if v > 0.05 => ActivityLevel::Medium, // 5-15% change
            _ => ActivityLevel::Low,                // <5% change
        }
    }
}

/// Main adaptive polling state manager
#[derive(Debug, Clone)]
pub struct AdaptivePollingState {
    pub base_interval: u64,
    current_interval: u64,
    enabled: bool,
    pub chat_tracker: ChatActivityTracker,
    pub metrics_tracker: MetricsVolatilityTracker,
    last_interval_update: Instant,
}

impl AdaptivePollingState {
    pub fn new(base_interval: u64, enabled: bool) -> Self {
        tracing::info!(
            enabled,
            base_interval,
            "adaptive polling system initialized"
        );

        Self {
            base_interval,
            current_interval: base_interval,
            enabled,
            chat_tracker: ChatActivityTracker::new(),
            metrics_tracker: MetricsVolatilityTracker::new(),
            last_interval_update: Instant::now(),
        }
    }

    pub fn register_chat_message(&mut self) {
        self.chat_tracker.register_message();
    }

    pub fn update_from_metrics(
        &mut self,
        viewers: Option<u64>,
        likes: Option<u64>,
        views: Option<u64>,
    ) {
        self.metrics_tracker
            .update_from_metrics(viewers, likes, views);
    }

    pub async fn calculate_optimal_interval(&mut self, outgoing: &mut TouchPortalHandle) -> u64 {
        if !self.enabled {
            outgoing
                .update_ytl_adaptive_polling_status(format!("{}s (Disabled)", self.base_interval))
                .await;
            return self.base_interval;
        }

        let chat_level = self.chat_tracker.calculate_activity_level();
        let metrics_volatility = self.metrics_tracker.calculate_volatility();

        let multiplier = match (chat_level, metrics_volatility) {
            (ActivityLevel::High, ActivityLevel::High) => 1.0, // Maximum responsiveness
            (ActivityLevel::High, _) => 2.5,                   // Chat provides real-time data
            (_, ActivityLevel::High) => 1.0,                   // Track rapid metrics changes
            (ActivityLevel::Medium, ActivityLevel::Medium) => 1.8, // Balanced monitoring
            (ActivityLevel::Low, ActivityLevel::Low) => 4.0,   // Minimal activity
            _ => 2.0,                                          // Default moderate adjustment
        };

        let optimal = (self.base_interval as f64 * multiplier) as u64;
        let new_interval = optimal.clamp(self.base_interval, self.base_interval * 6);

        self.last_interval_update = Instant::now();
        self.current_interval = new_interval;

        let reason = match (chat_level, metrics_volatility) {
            (ActivityLevel::High, ActivityLevel::High) => "Very Active",
            (ActivityLevel::High, _) => "Active Chat",
            (_, ActivityLevel::High) => "Changing Metrics",
            (ActivityLevel::Low, ActivityLevel::Low) => "Quiet",
            _ => "Normal",
        };

        let description = format!("{}s ({})", self.current_interval, reason);

        outgoing
            .update_ytl_adaptive_polling_status(description)
            .await;

        tracing::trace!(
            interval = new_interval,
            activity = ?chat_level,
            volatility = ?metrics_volatility,
            "adaptive polling interval calculated"
        );

        new_interval
    }
}
