# nginx auth — managing `.htpasswd`

The [`.htpasswd`](.htpasswd) file holds the HTTP Basic Auth credentials nginx uses to
protect the lab dashboard. Each line is `username:bcrypt-hash` — passwords are **never**
stored in plaintext, so you can't edit them by hand. Use the `htpasswd` tool to
generate the hashed entries.

> The default entry here is a placeholder (`admin`, copied from the live stack).
> **Rotate it before exposing the lab server** — it deploys to its own host with
> the same TLS + Basic-auth setup as live.

## Changing the username / password

### If you have `htpasswd` installed (Apache `httpd-tools`)

```bash
# Create or OVERWRITE the file with a single user (prompts for the password).
# -c = create (replaces the whole file!), -B = bcrypt ($2y$ format)
htpasswd -cB nginx/.htpasswd newuser

# Add or update a user WITHOUT wiping the existing file.
htpasswd -B nginx/.htpasswd newuser
```

- `-c` creates a fresh file — only use it for the first entry, it overwrites everything.
- `-B` forces bcrypt, matching the existing `$2y$...` format.
- It prompts for the password interactively, so the plaintext never lands in your shell history.

### If you don't have `htpasswd` but have Docker

```bash
# -n = print the user:hash line to stdout instead of writing a file
docker run --rm -it httpd:alpine htpasswd -nB newuser
```

Copy the printed `newuser:$2y$...` line into [`.htpasswd`](.htpasswd) (replacing the old line).

## Apply the change

nginx reads `.htpasswd` on each request, but reload to be safe / pick up config changes:

```bash
docker compose -f deploy/hunter-lab/compose.yml restart ui
```

## Notes

- Keep `.htpasswd` out of public reach; it should already be gitignored if it holds real creds.
- Current default user is `admin`. Rotate it before exposing the dashboard publicly.
