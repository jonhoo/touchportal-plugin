read this crate's source
read TARGET.md
read the README for an older YouTube TouchPortal plugin: <https://github.com/gitagogaming/Youtube-TouchPortal-Plugin/blob/master/README.md>
read these instructions for how to install a TouchPortal plugin: <https://www.touch-portal.com/blog/post/tutorials/import-plugin-guide.php>
read the Twitch plugin's entry.tp
read these documentation entries for the Twitch plugin:
- https://www.touch-portal.com/blog/post/tutorials/start-using-twitch-with-touch-portal-v3.php
- https://www.touch-portal.com/blog/post/tutorials/create-twitch-clip-and-share-it.php
- https://www.touch-portal.com/blog/post/tutorials/using-channel-points-redemption-redeem-event-in-touch-portal.php
- https://www.touch-portal.com/blog/post/tutorials/showing_twitch_viewers_followers_subscribers_of_twitch_on_button.php
- https://www.touch-portal.com/blog/post/tutorials/parsing-command-chat-messages-in-twitch-using-touch-portal.php
- https://www.touch-portal.com/blog/post/tutorials/show-welcome-message-to-new-chatter-this-live-session.php

Think about ways in which this plugin is similar to, and differs from, the other two plugins.

Write a README for the YouTube plugin that explains:
- What the plugin does.
- How to set up the plugin (authenticate an account, select a channel + broadcast, go live, wait for an event).
- How the plugin works for multi-channel streamers.
- The actions and events of the plugin. Make use of collapsible boxes to avoid a very long rendered page.
  - Highlight the fact that choosing "latest" as the broadcast will
    auto-attach to the newest non-completed broadcast, and will only
    disconnect again when that broadcast ends or when the plugin is
    restarted.
- A note on YouTube API quotas and limits, how the quota is shared by all users of the plugin, and why as a result users should contribute back to the plugin author according to their means and usage.
- A brief section on how this plugin differs from the Twitch one in a significant or surprising way.
- What is not supported (yet).
