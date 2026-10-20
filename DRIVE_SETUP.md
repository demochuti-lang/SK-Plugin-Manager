# Google Drive setup for SK Plugin Manager

SK Plugin Manager uploads backups to **your own** Google Drive using
**your own** Google Cloud credentials. Google requires this — there is no
shared key. It takes about 5 minutes, once.

## Steps

### 1. Create a Google Cloud project

1. Go to [Google Cloud Console](https://console.cloud.google.com/).
2. Sign in with the Google account whose Drive should hold the backups.
3. Click the project dropdown at the top → **New Project**.
4. Name it e.g. `SK Plugin Manager` → **Create**. Wait for it to finish,
   then select it in the project dropdown.

### 2. Enable the Google Drive API

1. In the left menu go to **APIs & Services → Library**.
2. Search for **Google Drive API** → open it → **Enable**.

### 3. Configure the OAuth consent screen

1. Go to **APIs & Services → OAuth consent screen**.
2. Choose **External** → **Create**.
3. Fill in:
   - **App name:** `SK Plugin Manager`
   - **User support email:** your email
   - **Developer contact information:** your email
4. **Save and Continue**.
5. On the **Scopes** page click **Add or Remove Scopes**, search for and
   check `https://www.googleapis.com/auth/drive.file`, then
   **Update → Save and Continue**.
   - This scope only lets the app see and manage **files it created
     itself** — it cannot read the rest of your Drive.
6. On the **Test users** page click **Add Users** and add your own Gmail
   address (the one from step 1), then **Save and Continue**.

> **Why "unverified app"?** Google only skips its warning for apps that
> complete their verification review. SK Plugin Manager is unpublished, so
> during sign-in Google shows an **"unverified app"** warning. This is
> expected: click **Advanced → Go to SK Plugin Manager (unsafe)** to
> continue. You are authorizing *your own* project, so your data stays
> yours.

### 4. Create the OAuth client ID (Desktop app)

1. Go to **APIs & Services → Credentials**.
2. Click **+ Create Credentials → OAuth client ID**.
3. **Application type:** choose **Desktop app**.
4. Name it e.g. `SK Plugin Manager Desktop` → **Create**.
5. A dialog shows your **Client ID** and **Client Secret** — copy both.
   (You can also download the JSON, but you only need the two strings.)

### 5. Paste them into SK Plugin Manager

1. Open SK Plugin Manager → **Settings → Backup → Google Drive**.
2. Paste the **Client ID** and **Client Secret** into the fields and save.
3. Click **Connect to Google Drive**.
4. Your browser opens the Google sign-in: choose your account, accept the
   "unverified app" warning as described above, and click **Allow**.
5. The browser tab says "Signed in to Google Drive" — close it and return
   to the app. The Settings screen now shows your account email.

## Using it

- **Back up now** with the **"Also upload to Google Drive"** toggle on, or
- In the Backups list, use the per-backup **Upload** action.

Uploaded backups show location **both** (local + Drive). Deleting such a
backup also removes its Drive copy.

## Disconnecting / revoking

- **Disconnect** in Settings clears the stored token from this computer.
- To fully revoke access: [myaccount.google.com](https://myaccount.google.com)
  → **Security → Third-party access** → remove **SK Plugin Manager**.
  Reconnecting afterwards gives you a fresh token.

## Troubleshooting

| Symptom | Fix |
|---|---|
| `NEEDS_SETUP: …` message | Client ID/Secret not saved yet — do step 5. |
| Google: "Access blocked" / Error 403 | Your account isn't a **test user** yet — redo step 3.6. |
| "Redirect URI mismatch" | The client must be type **Desktop app** (step 4), not Web. |
| Token refresh fails after a while | Revoke access (above) and Connect again. |
