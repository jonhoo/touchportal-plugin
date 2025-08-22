# Execution

This file describes how the YouTube Live TouchPortal plugin should
ultimately function. To get to the desired target state, follow this
plan:

1. Read the target state section below.
2. Invoke the TouchPortal agent to think about what actions, events, settings, and state need to be added to the plugin's manifest (in build.rs). Consider the TwitchTheSecond plugin and how it may inspire this plugin's design. Specifically highlight changes you're making to the plan based on the Twitch plugin.
3. Invoke the YouTube agent to think about whether the YouTube client API module has all the necessary APIs.
4. Consult the TouchPortal agent about how you will keep track of the current channel and stream, including across plugin restarts (you'll need to use a setting).
5. Implement all the settings.
6. Consult the YouTube agent to implement the main event loop that walks periodically sampled events (i.e., those polled) and continuous events (i.e., those from `streamList`).
7. Consult the TouchPortal agent and implement all the state updates and explicit events.
8. Consult the TouchPortal agent and implement all the actions. Make sure you handle the case where the user changes the stream or channel while the plugin is already running with a different channel/stream selected.

Make sure to read any linked resources.

Make use of SESSION.md.

Skip value storage things for now (but mention in SESSION.md).

# The Target State

There should be one "Select stream" action where you select a channel
(in case you have authorized multiple), and then a specific broadcast,
and all other actions and events operate on that broadcast implicitly.
The last selected channel and broadcast is saved in read-only settings
so that it is remembered if TouchPortal is restarted.

There should be a "latest" in the broadcast list that automatically
selects the latest non-completed broadcast for that channel.

We should include states for:

- the number of likes;
- the number of dislikes;
- the number of views; and
- the number of live viewers (if possible).

Since there is no way to stream these, they should be polled every X
seconds, where X should be a setting. The setting should have a minimum
value to avoid sending far too many requests to the YouTube API, since
the number of API calls that the app makes is limited
(<https://developers.google.com/youtube/v3/determine_quota_cost>). Once
we pass a certain number, I would have to start paying to maintain the
app. I'm not sure how to deal with that yet, but we should make that
fact visible to users of the plugin.

As updates are particularly expensive API calls, we should limit the
kinds of actions we support that result in updates. We need something to
start and stop a live broadcast, as well as for setting the title and
description. We should leave a TODO for setting the video thumbnail, for
sending chat messages, and for creating polls. I don't think there are
any other editing operations we should support at this time.

The "authenticate account" action should probably be called something
more user-friendly like "Add YouTube channel". When it's used, newly
authorized accounts should also become available in the "Select stream"
action I mentioned above.

To keep up with chat, we should use the `streamList` API. In particular, 
we should have one background task whose job it is to monitor the stream
event list of the currently selected stream. On plugin startup, this
task gets started, and pointed at the saved selected stream from
settings. Any time the "Select stream" action is executed, the monitor
should stop the old `streamList` and start a new one for the newly
selected stream.

Through monitoring the stream event list, we'll keep several states that
we update as stream events occur:

- the last chat message;
- the last chat message's author;
- the last super chat;
- the last super chat's author; and
- the last new sponsor.

An alternative to keep the last for each of these is to use events with
no value but with local state objects. That way, we can trigger those
when each even happens, and use the fields to communicate the various
bits of info related to the event like content, author, etc.

Whenever the selected stream changes, we should also update a state that
holds the title of the current stream.

We should also leave a TODO to use `activePollItem` to track the results
to a created poll, probably in a dynamically created state.

If possible, we should eventually also create states for the health
status, stream status, resolution, and framerate of the stream
associated with the currently selected live broadcast. These would also
be polled. For now though, those can have TODOs.

It may be worth checking the `entry.tp` manifest for the Twitch
TouchPortal plugin, which you can find in `TwitchTheSecond-entry.tp`. It
may have events, states, and actions they support that we can be
inspired by and apply to this plugin.

**NOTE: It is impossible to test this in an automated fashion. It will
require manual human testing.**
