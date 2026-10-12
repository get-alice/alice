---
layout: ../layouts/Legal.astro
title: Privacy policy
description: How the Alice website and the Alice email app handle your information. Alice has no server, no accounts and no tracking; your mail stays on your device.
updated: 2026-10-12
---

Alice is an open-source email client. This policy covers the website at getalice.email and the Alice apps for desktop and terminal. The short version: we don't run a server for your mail, we don't have accounts, and we don't track you.

## The website

The website is a set of static pages. It sets no cookies, runs no analytics or advertising scripts, and loads no third-party trackers. Fonts are served from this site.

The site is hosted by [Vercel](https://vercel.com/legal/privacy-policy). Like any web host, Vercel receives your IP address and browser details when you visit, and keeps short-lived server logs to deliver the site and protect it from abuse. We don't use those logs to identify or profile visitors.

Download links point to [GitHub](https://docs.github.com/en/site-policy/privacy-policies/github-general-privacy-statement), which serves the installers under its own privacy statement.

## The app

### What stays on your device

Alice runs a small service on your computer that connects to your mail provider and keeps a copy of your mail, drafts, settings and recipient suggestions in a local database. Nothing is sent to us. The data lives in your user folder:

- macOS: `~/Library/Application Support/Alice`
- Windows: `%LOCALAPPDATA%\Alice`
- Linux: `~/.local/share/Alice`

Passwords, sign-in tokens and AI API keys are kept in your system's credential store: macOS Keychain, Windows Credential Manager or the Linux Secret Service. They are never written to the database.

Alice has no telemetry, crash reporting or usage analytics.

### Who Alice talks to

Alice connects only to services you set up, and only to do what you ask:

- **Your mail provider**, over encrypted IMAP and SMTP, to fetch, send and organize your mail. When you sign in with Google or Microsoft, the sign-in happens in your browser and the tokens are stored in your credential store.
- **Address books and calendars you connect**, such as Google, Outlook, CardDAV or CalDAV servers, or Mac Contacts. Address books are read only. Alice adds a calendar event only when you choose to add one.
- **The AI provider you choose**, only if you set up Ask Alice. Alice sends the message you ask about, or your newest 20 inbox messages for questions about the inbox, with each message capped at 12,000 characters. If a calendar is connected, it also sends the current time and your busy times for the next two weeks, without event titles. Requests go straight from your device to that provider under its terms, using your API key. You can use a model running on your own computer instead.
- **Senders' image servers**, only when you choose to load remote images in a message. Until then, those images are blocked, so a sender can't tell when you open their email.
- **An update feed**, only if one is configured. Alice then downloads a small file that names the latest version.

Those services handle your data under their own privacy policies.

### Google user data

Alice's use and transfer of information received from Google APIs adheres to the [Google API Services User Data Policy](https://developers.google.com/terms/api-services-user-data-policy), including the Limited Use requirements. Alice uses Gmail, contacts and calendar data only to provide the features you see in the app. That data stays on your device, except when you ask Alice's AI feature about your mail, which sends the messages described above to the AI provider you chose. We never sell Google user data, use it for advertising, or use it to train AI models.

### Removing your data

Removing an account in **Settings → Mail accounts**, or with `alice accounts remove`, deletes its cached mail, unsent drafts and stored sign-in from your device. Your mail on the provider's server is not affected. To remove everything, uninstall Alice and delete the folder listed above. You can also revoke Alice's access from your Google or Microsoft account settings at any time.

## Children

Alice is not directed at children under 13, and we don't knowingly collect information from anyone.

## Changes

If this policy changes, we'll update this page and the date at the top. Because Alice is open source, every change to the app's behavior is also visible in its [public code history](https://github.com/get-alice/alice).

## Contact

Questions about privacy: [privacy@getalice.email](mailto:privacy@getalice.email), or open an issue on [GitHub](https://github.com/get-alice/alice/issues).
