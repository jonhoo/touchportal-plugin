# YouTube Live TouchPortal Plugin

Control your YouTube Live streams directly from TouchPortal on your mobile device or desktop! This plugin brings YouTube Live streaming controls, real-time analytics, and chat monitoring right to your fingertips.

## What Does This Plugin Do?

Whether you're a seasoned content creator or just getting started with live streaming, this plugin transforms TouchPortal into your YouTube Live command center. Monitor your viewer counts, track chat messages and Super Chats, control your broadcasts, and keep tabs on your stream metrics - all without leaving your streaming setup.

**Key Features:**
- 🎛️ **Stream Control**: Start/stop broadcasts, update titles and descriptions
- 📊 **Real-Time Analytics**: Live viewer counts, likes, dislikes, and view tracking
- 💬 **Chat Monitoring**: Track messages, Super Chats, and new channel members
- 🔄 **Multi-Channel Support**: Manage multiple YouTube channels from one plugin
- ⚡ **Smart Polling**: Adaptive API usage that optimizes based on your streaming activity
- 🎯 **"Latest Broadcast" Mode**: Automatically connects to your newest stream

## Installation & Getting Started

### Prerequisites
- TouchPortal Pro (plugins are not available in the free version)
- A YouTube channel with live streaming enabled
- Windows, macOS, or Linux computer running TouchPortal

### Step-by-Step Installation

1. **Download the Plugin**
   - Download the latest `.tpp` file from the [releases page](https://github.com/jonhoo/touchportal-plugin/releases)

2. **Import into TouchPortal**
   - Open TouchPortal and click the wrench icon (settings)
   - Select "Import plug-in..."
   - Choose the downloaded `.tpp` file
   - **Important**: Review the plugin permissions carefully before accepting

3. **Restart TouchPortal**
   - Completely restart TouchPortal from the system tray
   - The YouTube Live plugin should now appear in your plugin list

4. **First-time setup**
   - Use the "Add YouTube channel" action to authenticate your account
   - Use the "Select stream" action to choose which broadcast to monitor
   - You're ready to start streaming with TouchPortal control!

## Authentication & multi-channel setup

### How authentication works

This plugin uses OAuth 2.0 authentication - the same secure method YouTube uses for official apps. When you authenticate:

1. The plugin opens your web browser to YouTube's official login page
2. You sign in with your Google account (no passwords stored locally)
3. YouTube asks if you want to grant the plugin access to your channel
4. Once approved, you're redirected back and ready to stream

### Adding multiple channels

Perfect for creators who manage multiple YouTube channels! Each channel gets its own authentication:

1. Use the "Add YouTube channel" action for each additional channel
2. Complete the OAuth flow for each account
3. Use "Select stream" to switch between channels seamlessly
4. All channels remain authenticated until you revoke access

The plugin remembers your channels between TouchPortal restarts, so you only need to authenticate once per channel.

## Actions and events

<details>
<summary>🎛️ Stream management actions</summary>

### Add YouTube channel action
Authenticate and add your YouTube channels for multi-account management.

### Select stream action
The heart of the plugin - choose which broadcast to monitor and control:

- **Channel Selection**: Pick from all your authenticated YouTube channels
- **Broadcast Selection**: Choose from your upcoming, live, or recent broadcasts
- **"Latest non-completed broadcast"**: ⭐ **Special feature** - automatically attaches to your newest active broadcast and stays connected until that broadcast ends or the plugin restarts

### Broadcast control actions
- **Start Live Broadcast**: Begin streaming to your selected broadcast
- **Stop Live Broadcast**: End your current live stream
- **Update Stream Title**: Change your stream title on the fly
- **Update Stream Description**: Modify your stream description while live

### Settings & configuration
- **Smart polling adjustment**: Automatically optimizes API usage based on stream activity
- **Base polling interval (seconds)**: Control how often the plugin checks for stream metrics (30-3600 seconds) - note this doesn't affect chat monitoring frequency
- **Custom OAuth client ID** and **Custom OAuth client secret**: For heavy users who want dedicated API quota (see [QUOTA.md](./QUOTA.md))

</details>

<details>
<summary>📊 Real-time analytics & monitoring states</summary>

### Live stream metrics states
Track your stream's performance in real-time through these TouchPortal states:

- **YouTube Live - live viewers**: Current concurrent viewer count
- **YouTube Live - views**: Total video view count
- **YouTube Live - likes**: Number of likes on your stream
- **YouTube Live - dislikes**: Number of dislikes on your stream
- **YouTube Live - chat messages**: Total chat message count

### Stream information states
Current stream and channel details:
- **YouTube Live - stream title**: Title of your currently selected broadcast
- **YouTube Live - channel name**: Name of your currently selected channel

### System status states
Plugin operation information:
- **YouTube Live - adaptive polling status**: Shows current polling optimization status

</details>

<details>
<summary>💬 Chat & community events</summary>

### Chat & community events
The plugin provides these events that you can use to trigger actions in TouchPortal:

#### Action-based events (with rich local state data):
- **On new chat message**: Triggers when you receive a new chat message
  - Includes message content, author name, author ID, and timestamp

- **On new Super Chat**: Triggers when you receive a Super Chat
  - Includes message content, author, amount in display format and micros, and currency

- **On new member**: Triggers when your channel gets a new member
  - Includes member name and membership level

- **On new member milestone**: Triggers when an existing member reaches a new milestone
  - Includes milestone member name, level, and the number of months that user has been a member

### Chat event states
These states track the most recent chat activity:
- **YouTube Live - last chat message**: Content of the most recent chat message
- **YouTube Live - last Super Chat**: Content of the most recent Super Chat
- **YouTube Live - last member**: Name of the most recent new member
- **YouTube Live - last chat author**: Author of the most recent chat message
- **YouTube Live - last Super Chat author**: Author of the most recent Super Chat
- **YouTube Live - last Super Chat amount**: Amount of the most recent Super Chat
- **YouTube Live - last member tenure**: Tenure of the most recent member
- **YouTube Live - last member level**: Membership level of the most recent member

Additionally, there are corresponding "changed" events that trigger when each of these states updates.

</details>

## YouTube API quotas & community contribution

### Understanding the shared system

Here's something important to know: **YouTube's API has daily limits**, and by default, all users of this plugin share the same quota pool of 10,000 requests per day.

Think of it like a shared internet connection - if one person downloads huge files all day, everyone else gets slower internet. Similarly, if plugin users check their stats very frequently, it can use up the shared quota for everyone.

### Why this matters

When the shared quota is exhausted (which resets at midnight Pacific Time):
- **Nobody can use the plugin** until the next day
- Chat monitoring stops working
- Stream metrics can't be updated
- Broadcast controls become unavailable

This isn't a limitation we chose - it's how Google's YouTube API works for shared applications.

### How you can help

**Be mindful of your polling settings:**
- Use 60+ second intervals for casual monitoring
- Enable "Smart polling adjustment" to optimize usage automatically
- Consider your actual needs - do you really need updates every 30 seconds?

**For heavy users:**
If you stream frequently or want very frequent updates, consider setting up [custom OAuth credentials](./QUOTA.md). This gives you:
- Your own private 10,000 daily quota
- No impact from other users' usage
- More reliable access to your streaming data
- **Bonus**: Helps reduce load on the shared quota for other users

**Community spirit:**
The more users who set up their own credentials, the better the experience becomes for everyone still using the shared system. It's a win-win!

### Quota usage examples
Here's what different activities cost:
- Checking if you're live: ~3 units per check
- Getting viewer count: ~1 unit per check
- Reading chat messages: ~1 unit per check
- Updating stream title: ~50 units per operation

A 4-hour stream checking stats every 60 seconds uses roughly 1,200 units - well within the daily limit for individual users.

## How this differs from Twitch plugins

While both platforms serve the streaming community, YouTube Live has some unique characteristics that shape how this plugin works:

### YouTube-specific features
- **Super Chats**: YouTube's monetization system is different from Twitch's bits/subscriptions
- **Channel Memberships**: YouTube's membership system with tiers and milestones
- **Broadcast Lifecycle**: YouTube has a more complex broadcast creation and management system

### Key differences for users
- **Broadcast Selection**: YouTube requires explicit broadcast selection (unlike Twitch's single-stream model)
- **"Latest Broadcast" Feature**: Unique to YouTube's multi-broadcast system
- **Different Monetization Events**: Super Chats instead of bits, memberships instead of subscriptions
- **Community Features**: YouTube's community posts and membership system offer different engagement options

### What this means for users
- Setup involves selecting specific broadcasts rather than just connecting to "your stream"
- More granular control over which stream content you're monitoring
- Different monetization event types (Super Chats vs. bits)
- Enhanced multi-channel support due to YouTube's account structure

## What's not supported yet

While this plugin covers the most important YouTube Live features, some capabilities are intentionally not included due to API limitations and costs:

### Chat interaction features
**Why not included**: These operations are expensive (50+ API units each) and would quickly exhaust the shared quota.

- ❌ Sending chat messages
- ❌ Deleting chat messages
- ❌ Timing out or banning users
- ❌ Moderator actions (beyond monitoring)

### Advanced stream management
**Status**: Planned for future updates

- ❌ Custom video thumbnail updates
- ❌ Community post creation
- ❌ Poll creation and management
- ❌ Stream category/game changes

### Technical stream data
**Status**: Under consideration

- ❌ Stream health and quality metrics
- ❌ Encoder settings information
- ❌ Network performance data
- ❌ Advanced analytics beyond basic metrics

### Future possibilities
Some of these features might be added in future versions, especially:
- Poll support (when YouTube's API becomes more accessible)
- Thumbnail updates (if demand is high enough)
- Advanced chat features (for users with custom credentials)

The current feature set focuses on the most valuable, cost-effective operations that benefit the entire community.

**Need one of these features specifically?** Please reach out via GitHub issues or the TouchPortal Discord - we're always evaluating what to prioritize based on community needs.

## Troubleshooting & technical notes

<details>
<summary>Common setup issues</summary>

### "No channels available" in stream selection
- **Solution**: Use the "Add YouTube channel" action first to authenticate
- **Check**: Make sure your YouTube channel has live streaming enabled
- **Verify**: Complete the OAuth flow in your browser when prompted

### "Select channel first" for broadcast selection
- **Solution**: Choose a channel in the dropdown before selecting a broadcast
- **Note**: The plugin needs to know which channel to check for broadcasts

### Authentication browser window doesn't open
- **Check**: Your default browser settings
- **Try**: Restarting TouchPortal and trying the action again
- **Alternative**: Check if your firewall or antivirus is blocking the OAuth flow

</details>

<details>
<summary>Performance & quota issues</summary>

### "API quota exceeded" errors
- **Immediate**: Wait until midnight Pacific Time for quota reset
- **Long-term**: Consider [custom OAuth credentials](./QUOTA.md)
- **Optimize**: Increase your polling interval to reduce API usage

### Plugin seems slow or unresponsive
- **Check**: Your internet connection stability
- **Verify**: YouTube's API service status
- **Adjust**: Polling intervals - very low values can cause API throttling

### Missing chat messages or events
- **Cause**: Rapid chat during popular streams may exceed API rate limits
- **Solution**: The plugin prioritizes recent messages over historical ones
- **Note**: This is a YouTube API limitation, not a plugin bug

</details>


## Getting help & contributing

### Need support?

**Before opening an issue**, please check:
1. Your TouchPortal version (Pro required)
2. Your internet connection
3. YouTube's service status
4. The troubleshooting section above

**Enable detailed logging** through the plugin settings for better support:
- **Info**: Basic operation messages
- **Debug**: Detailed operation information
- **Trace**: Comprehensive debugging including API calls

**For bug reports and feature requests:**
- Visit our [GitHub repository](https://github.com/jonhoo/touchportal-plugin)
- Join the [TouchPortal Discord](https://discord.gg/MgxQb8r) for community help
- Provide detailed information about your setup
- Include relevant log output when reporting issues

### Contributing to the community

This plugin exists thanks to community contributions:

**Share the API quota load:**
- Set up [custom credentials](./QUOTA.md) if you're a frequent user
- This helps keep the shared quota available for newcomers and casual users

**Technical contributions:**
- Report bugs with detailed reproduction steps
- Suggest features that would benefit the YouTube creator community
- Contribute code improvements via pull requests

**Community guidelines:**
- Be respectful and helpful to other users
- Share knowledge about streaming setups and best practices
- Help newcomers get started with the plugin
