# Online storage: what the maintainer has to do

Spark Commander reaches online storage through its bundled rclone. Six of
the services log in through the browser (OAuth). For those, each provider
wants the app registered with it once: the registration gives a **client
ID** and a **client secret**, which go into our builds. rclone's own IDs
belong to rclone and are not ours to ship (Google is about to charge rclone
for its one), so every service below needs its own registration.

Already working, no registration: **S3** (AWS, Cloudflare R2, Wasabi, …).
No registration, but not built yet: **iCloud Drive, Mega, Proton Drive**
(they log in with the account's own password). For those, only test
accounts are needed.

Checked 2026-11-15. Items marked *unconfirmed* come from forums or
third-party pages, not from the provider.

## At a glance

| Service | Where | Account you need | Cost | Before many users can use it |
|---|---|---|---|---|
| Dropbox | dropbox.com/developers/apps | any Dropbox account | free | production approval once 50 users have linked |
| Box | app.box.com/developers/console | any Box account | free | nothing (company Box admins may have to allow it) |
| Yandex Disk | oauth.yandex.com | a Yandex ID | free | developer verification, or users see a warning |
| OneDrive | entra.microsoft.com | an Entra tenant (free Azure account; needs a card for the identity check) | free | publisher verification (optional, needs your own domain) |
| pCloud | e-mail support@pcloud.com | a pCloud account | free | nothing known |
| Google Drive | console.cloud.google.com | any Google account | free to start; verification may cost ~$540–855 a year | Google's verification of the `drive` scope |

Suggested order: Dropbox, Box and Yandex (10–15 minutes each), then
Microsoft, then the pCloud e-mail, and Google last.

## What every registration asks for

Have these ready:

- **App name:** "Spark Commander". Dropbox needs a name nobody else uses, so have a second choice ready.
- **Homepage:** https://github.com/remoxio/spark-commander
- **Privacy policy page:** Google, and Dropbox's production review, ask for one. A short page on GitHub Pages is enough. It can say the app keeps the login tokens on the user's computer, in the system keyring, and sends files only to the service the user picked.
- **Icon:** the Lightning icon from the icon pack.
- **Support e-mail:** one you are happy to show to users on the login screen.

The **redirect URI** must be typed exactly as below, slash included. It is
the address rclone listens on while the browser login runs:

| Service | Redirect URI |
|---|---|
| Google Drive | none to enter (a "Desktop app" client takes any 127.0.0.1 port) |
| OneDrive | `http://localhost:53682/` |
| Dropbox | `http://localhost:53682/` |
| Box | `http://127.0.0.1:53682/` |
| pCloud | `http://localhost:53682/` |
| Yandex Disk | `http://127.0.0.1:53682/` |

## Dropbox

1. Go to https://www.dropbox.com/developers/apps and choose **Create app**.
2. Pick **Scoped access**, then **Full Dropbox**, and give the name.
3. On the **Settings** tab, under *Redirect URIs*, add `http://localhost:53682/`.
4. On the **Permissions** tab, tick: `account_info.read`,
   `files.metadata.write`, `files.content.write`, `files.content.read`,
   `sharing.write`. Then press **Submit**.
5. Keep the **App key** (the client ID) and the **App secret**.

Limits: a new app is in *development* mode and can link up to 500 users.
Once 50 users have linked, you have **2 weeks** to apply for production
(Settings → *Apply for production*: icon, description, privacy policy).
Otherwise no new users can link. Ask for no more permissions than above:
the review turns down broader ones.
([Dropbox developer guide](https://docs.dropboxapi.com/dropbox-api/docs/developer-resources/developer-guide), [rclone's Dropbox page](https://rclone.org/dropbox/))

## Box

1. Go to https://app.box.com/developers/console, then **Create Platform App** → **Custom App**.
2. Pick **User Authentication (OAuth 2.0)**. As purpose, "automation" asks the fewest questions.
3. On **Configuration**:
   - *Redirect URI:* `http://127.0.0.1:53682/`
   - *Application Scopes:* tick **Read and write all files and folders stored in Box**.
   - Save.
4. Keep the **Client ID** and the **Client Secret**.

No approval is needed. People with free and personal Box accounts can log in
straight away. A company's Box admin may have blocked unpublished apps; they
can allow ours by its client ID. Listing the app in Box's integrations
directory is optional.
([Box: OAuth 2.0 setup](https://developer.box.com/guides/authentication/oauth2/oauth2-setup), [Box: app approval](https://developer.box.com/guides/authorization/platform-app-approval))

Note: Box's login lasts 60 days without use. Someone who doesn't open their
Box connection for 60 days has to log in again.

## Yandex Disk

1. Go to https://oauth.yandex.com/client/new, signed in with a Yandex ID.
2. Give the name. As type, choose **For user authorization** (it can't be changed later).
3. As platform, choose **Web services**, with the redirect URI `http://127.0.0.1:53682/`.
   - *Unconfirmed:* Yandex's documentation doesn't say whether a `http://127.0.0.1` address is accepted. rclone's author recommended it.
4. Under data access, *Yandex.Disk REST API*, tick `cloud_api:disk.read`,
   `cloud_api:disk.write` and `cloud_api:disk.info`. Don't tick only
   `app_folder`: that limits the app to one folder.
5. Keep the **ClientID** and the **Client secret**.

Until your Yandex developer account is verified, users see a warning when
they log in. Outside Russia, verification goes through Yandex support.
([Yandex ID: registering an app](https://yandex.com/dev/id/doc/en/register-auth), [Yandex ID: verification](https://yandex.com/dev/id/doc/en/confirm-account))

## OneDrive (Microsoft)

A personal Microsoft account alone can no longer register apps: it needs an
**Entra tenant** (Microsoft's directory). The free Azure account creates one.
Signing up needs a phone number and a credit or debit card (not a prepaid
one) for the identity check. Nothing is charged unless you switch to
pay-as-you-go yourself. After 30 days the free subscription stops, but the
tenant stays.
- *Unconfirmed:* whether app registrations keep working without an active subscription.

1. Sign up at https://azure.microsoft.com/free, then go to https://entra.microsoft.com.
2. Go to **Entra ID → App registrations → New registration**:
   - *Name:* Spark Commander.
   - *Supported account types:* **Accounts in any organizational directory and personal Microsoft accounts**.
   - *Redirect URI:* platform **Web**, `http://localhost:53682/`.
3. Go to **Certificates & secrets → New client secret**:
   - Choose the longest expiry, **24 months**.
   - Copy the **Value** right away (not the "Secret ID"). It is shown only once.
4. Go to **API permissions → Add → Microsoft Graph → Delegated**, and add:
   `Files.Read`, `Files.ReadWrite`, `Files.Read.All`, `Files.ReadWrite.All`,
   `Sites.Read.All`, `offline_access`, `User.Read`.
5. Keep the **Application (client) ID** and the secret's **Value**.

Important:

- **The secret expires** after 24 months at most. Before then, a new secret
  has to be made and new builds shipped, or OneDrive stops working for
  everyone. Put a reminder in your calendar.
- **Work and school accounts:** since August 2025, new company tenants don't
  let their users approve the `…All` and `Sites…` permissions; only an admin
  can. For those accounts the app will ask for fewer permissions, which only
  loses "Shared with me". Personal OneDrive is not affected.
  ([Microsoft: consent policies](https://learn.microsoft.com/en-us/entra/identity/enterprise-apps/manage-app-consent-policies))
- **Publisher verification** (optional: it removes "unverified" from the
  login screen). It is free, but needs:
  - a Microsoft AI Cloud Partner Program account;
  - your own domain, verified in the tenant;
  - the app registered from a work account of that tenant.

  Leave it for later.
  ([Microsoft: publisher verification](https://learn.microsoft.com/en-us/entra/identity-platform/publisher-verification-overview))

([rclone's OneDrive page](https://rclone.org/onedrive/))

## pCloud

pCloud's self-service app registration is reported closed. Apps are
requested by e-mail instead (*unconfirmed*: from third-party pages, pCloud's
own documentation is silent). A draft:

> To: support@pcloud.com
> Subject: OAuth app registration request – Spark Commander
>
> Hello, I develop Spark Commander, a free, open-source (GPL-3.0) twin-panel
> file manager for Linux, Windows and macOS
> (https://github.com/remoxio/spark-commander). I would like to let its users
> open their pCloud storage in the app, through rclone. Could you please
> create an OAuth 2.0 app for it and send me its client ID and client secret?
> Redirect URI: http://localhost:53682/
> Thank you.

## Google Drive

This is the hard one, so start it last. Registering is easy. Letting the
public use it is not: the app needs the `drive` scope (full access), which
Google calls *restricted*.

1. Go to https://console.cloud.google.com and create a project, "Spark Commander".
2. **APIs & Services → Library**: enable **Google Drive API**.
3. Set up **Google Auth Platform**:
   - *Branding:* name, support e-mail, homepage, privacy policy.
   - *Audience:* **External**.
   - *Data access:* add `https://www.googleapis.com/auth/drive`.
4. **Clients → Create client**, type **Desktop app**. Copy the **client
   secret** at once: since 2025 Google shows it only when it is created.
5. **Audience → Test users:** add the Google accounts that test it (up to 100).

The stages:

| Stage | Who can log in | The catch |
|---|---|---|
| Testing | up to 100 test users you list | logins expire after **7 days** |
| In production, not verified | the first **100 users, ever** (it never resets) | a warning screen at login |
| Verified | everyone | verification is redone every 12 months |

Verification steps:

- Brand check (2–3 days).
- Restricted-scope review: a short video of the login and of what the app
  does with the files, plus why it needs full access. This takes weeks.
- Possibly a **CASA security assessment** by a Google-approved lab, about
  $540–855 a year at the lowest tier.
  - *Unconfirmed:* whether a desktop app that sends nothing to its own
    servers needs CASA. Google's wording suggests it may not.
  - rclone's author: their verification "took 3 months and $600 last time".

([Google: restricted-scope verification](https://developers.google.com/identity/protocols/oauth2/production-readiness/restricted-scope-verification), [Google: unverified apps](https://support.google.com/cloud/answer/15549945), [rclone forum, July 2026](https://forum.rclone.org/t/google-drive-and-google-photos-users-action-required/54005))

Google is also starting to bill Drive API use above a free daily allowance,
"later in 2026" with 90 days' notice. The allowance (400 million quota units
a day per project) is far above what a new app uses.
([Drive API limits](https://developers.google.com/workspace/drive/api/guides/limits))

My suggestion: register it and keep it in **Testing** while we build it and
test it with your own accounts. Decide about verification before a public
release. Until then the app can also take a user's own Google client ID, as
rclone now asks its users to do.

## Keeping the keys out of the public repo

Google's rules say client credentials must never be committed to a public
repository, and that is good practice for the others too. So don't put them
in the repo, and don't paste them into the chat. Put them in a file outside
the repo, readable only by you:

```sh
mkdir -p ~/.config/sparkcommander-dev
touch ~/.config/sparkcommander-dev/oauth.env
chmod 600 ~/.config/sparkcommander-dev/oauth.env
```

```sh
# ~/.config/sparkcommander-dev/oauth.env
SC_DROPBOX_CLIENT_ID=...
SC_DROPBOX_CLIENT_SECRET=...
SC_BOX_CLIENT_ID=...
SC_BOX_CLIENT_SECRET=...
SC_YANDEX_CLIENT_ID=...
SC_YANDEX_CLIENT_SECRET=...
SC_ONEDRIVE_CLIENT_ID=...
SC_ONEDRIVE_CLIENT_SECRET=...      # expires: write the date here too
SC_PCLOUD_CLIENT_ID=...
SC_PCLOUD_CLIENT_SECRET=...
SC_GOOGLE_CLIENT_ID=...
SC_GOOGLE_CLIENT_SECRET=...
```

The build reads them from there and compiles them into the app. A build
without them simply doesn't offer those services. For release builds (store
versions, GitHub releases), the same values go into the build machine's
secret settings.

As with every desktop app, rclone's included, a determined person can dig
the values out of the program. The providers know this about desktop apps.
It only lets someone pose as our app at login. It gives no access to any
user's files.

## Test accounts

Each service needs one account to test with; free plans are enough. You log
in once in the browser when I ask, and I never type your passwords. For
iCloud Drive, Mega and Proton Drive, which are still to build, test accounts
are all that's needed:

- **iCloud Drive:** "Access iCloud Data on the Web" must be on. Logins last 30 days, then a new 2FA code.
- **Mega:** logging in too often can block the account for about a week.
- **Proton Drive:** rclone's support for it is still beta.

## What I do once the keys are there

- Make the build read the keys, as above.
- Add each service to the Connections dialog: the browser login runs
  through the bundled rclone. The login tokens are kept in rclone's own
  settings file, encrypted, with its password in the system keyring.
- Update the bundled rclone to 1.76 or newer. With 1.75.2, OneDrive for
  Business and SharePoint transfers stop working on 1 April 2027
  ([rclone's OneDrive page](https://rclone.org/onedrive/)).
- Ask OneDrive work accounts for fewer permissions, so no admin is needed.
- Test each service with your test accounts.
