# YouTube API Quota and Custom Credentials

## What's This About?

When you use the YouTube Live TouchPortal plugin, it needs to communicate with YouTube's servers to get information about your streams, chat messages, and viewer counts. Google (who owns YouTube) limits how many requests can be made per day through their **YouTube Data API v3** - these are called "quota limits."

Think of it like having a daily allowance of internet requests. The default allowance is **10,000 units per day**, which resets every night at midnight Pacific Time.

## The Shared Credentials Challenge

By default, this plugin comes with built-in credentials that all users share. This means:

- **All plugin users worldwide share the same 10,000 daily quota**
- If someone uses the plugin heavily (checking stats every 30 seconds), they might use up the quota for everyone
- When the quota runs out, **nobody** can use the plugin until it resets at midnight

This isn't a limitation we chose - it's how Google's API system works. While larger applications can request higher limits through Google's [compliance audit process](https://developers.google.com/youtube/v3/guides/quota_and_compliance_audits), this is challenging for open source projects and can take months to approve. I am actively pursuing increased limits for the shared credentials, but the process is arduous.

## When Do You Need Your Own Credentials?

You should consider setting up your own credentials if:

- ✅ You stream frequently (multiple times per week)
- ✅ You want to check your stats often (every minute or less)
- ✅ You're experiencing "quota exceeded" errors
- ✅ You want guaranteed access without depending on other users' usage
- ✅ You're a power user who wants the most reliable experience
- ✅ You have the technical know-how and want to help reduce stress on the shared quota for other users

## What Are Custom Credentials?

Creating your own credentials is like getting your own dedicated internet connection instead of sharing one with your neighbors. You get:

- **Your own private 10,000 daily quota** (all to yourself!)
- **No impact from other users** - their usage won't affect you
- **No impact on other users** - your usage won't affect them
- **More reliable access** to your streaming data

## Why Many Projects Require This

This isn't unique to our plugin. Successful open source projects that use YouTube's API require users to create their own credentials:

- [YouTube Super Chat Monitor](https://github.com/grantwilk/youtube-super-chat-monitor) - "This library does not come packaged with an API key or OAuth Client ID"
- [Reddit2Tube](https://github.com/roperi/Reddit2Tube) - Requires users to create their own Google Cloud project
- [YouTube Shorts Automation](https://github.com/HalmonLui/YoutubeShortsAutomation) - Users must set up their own OAuth credentials

This is standard practice because it's the only way to ensure reliable service for everyone.

## Step-by-Step Setup Guide

**Estimated time: 10-15 minutes (one-time setup)**

### Step 1: Create a Google Cloud Account

1. Go to [Google Cloud Console](https://console.cloud.google.com/)
2. Sign in with your Google account (the same one you use for YouTube)
3. Accept the terms of service if prompted

### Step 2: Create a New Project

1. Click the project dropdown at the top of the page
2. Click "New Project"
3. Give it a name like "My YouTube Plugin" or "TouchPortal YouTube"
4. Click "Create"
5. Wait for the project to be created, then select it

### Step 3: Enable the YouTube Data API v3

1. In the left sidebar, go to "APIs & Services" > "Library"
2. Search for "YouTube Data API v3"
3. Click on it and press "Enable"
4. Wait for it to enable (this might take a moment)

### Step 4: Configure OAuth Consent Screen

1. Go to "APIs & Services" > "OAuth consent screen"
2. Choose "External" (unless you have a Google Workspace account, then you can choose "Internal")
3. Fill out the required information:
   - **App name**: "TouchPortal YouTube Plugin" (or similar)
   - **User support email**: Your email address
   - **Developer contact information**: Your email address
4. Click "Save and Continue"
5. On the "Scopes" page, click "Add or Remove Scopes"
6. Search for and add: `https://www.googleapis.com/auth/youtube`
7. Click "Update" then "Save and Continue"
8. On the "Test users" page, add your own email address as a test user
9. Click "Save and Continue"

### Step 5: Create OAuth 2.0 Credentials

1. Go to "APIs & Services" > "Credentials"
2. Click "Create Credentials" > "OAuth client ID"
3. Choose "Desktop application" as the application type
4. Give it a name like "TouchPortal YouTube Plugin"
5. Click "Create"

### Step 6: Download Your Credentials

1. After creating, you'll see a dialog with your Client ID and Client Secret
2. **Copy both values** - you'll need them for the plugin
3. You can also click "Download JSON" to save them to a file for backup

### Step 7: Configure the Plugin

1. Open TouchPortal
2. Go to the YouTube Live plugin settings
3. Paste your **Client ID** into the "Custom OAuth Client ID" field
4. Paste your **Client Secret** into the "Custom OAuth Client Secret" field
5. Save the settings

### Step 8: Re-authenticate

1. Use the "Authenticate with YouTube" action to log in again
2. You'll now be using your own dedicated API quota!

## Understanding Quota Usage

Here's what different plugin activities cost in terms of your daily 10,000 quota:

- **Checking if you're live**: ~3 units per check
- **Getting viewer count**: ~1 unit per check  
- **Reading chat messages**: ~1 unit per check
- **Getting stream info**: ~1 unit per check
- **Update operations** (changing stream titles, etc.): ~50+ units per operation

If you check your stats every 60 seconds during a typical 4-hour stream, you'll use roughly:
- **5 units per minute** × 60 minutes × 4 hours = **1,200 units per stream**

The plugin's **adaptive polling** feature helps optimize usage by checking more frequently when your stream is active and less often when idle, helping you stay within the 10,000 daily limit.

## Security and Privacy

When you create your own credentials:

- ✅ **Google cannot see your plugin usage** beyond normal API monitoring
- ✅ **Your credentials only work for your Google account**
- ✅ **No one else can use your credentials** (they're tied to your project)
- ✅ **You can revoke access at any time** through Google Cloud Console
- ✅ **The plugin only requests the minimum permissions needed** (`youtube` scope)

## Troubleshooting

### "OAuth consent screen needs verification"
If you see this message, don't worry! For personal use, you can:
1. Add yourself as a test user (Step 4 above)
2. Continue using the app - it will work fine for your own account

### "Invalid client ID or secret"
- Double-check you copied the values correctly (no extra spaces)
- Make sure you're using a "Desktop application" type credential
- Verify the YouTube Data API v3 is enabled for your project

### "Quota exceeded" still happening
- Wait until midnight Pacific Time for your quota to reset
- Check your polling interval - lower values use more quota
- Verify you're using your custom credentials (re-authenticate if needed)


## FAQ

**Q: Will this cost me money?**  
A: No! Google provides 10,000 free API units per day. Most users never exceed this limit.

**Q: Do I need to do this setup again?**  
A: No, this is a one-time setup. Your credentials will work indefinitely.

**Q: Can I share my credentials with friends?**  
A: Please don't - each user should create their own. Sharing defeats the purpose of having separate quotas.

**Q: What if I already have a Google Cloud project?**  
A: You can use an existing project! Just enable the YouTube Data API v3 and create new OAuth credentials.

**Q: Is this really necessary?**  
A: Only if you're a heavy user or experiencing quota issues. Casual users can continue using the shared credentials.

---

**Still need help?** Feel free to open an issue on the [GitHub repository](https://github.com/jonhoo/touchportal-plugin) with your questions!