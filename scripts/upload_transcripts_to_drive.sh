#!/usr/bin/env bash
# Upload Claude Code session/subagent transcripts to a flat Google Drive folder.
#
# Filenames are kept exactly as-is (session UUID or "agent-<id>.jsonl") since
# they're already globally unique identifiers — the RDF graph (agp:transcriptRef,
# see docs/plans/AGENT_PROVENANCE_PLAN.md) is what provides navigation, not
# folder structure. See issue #333.
#
# One-time setup (run this yourself first, it's interactive):
#   1. Install rclone: https://rclone.org/install/  (e.g. `sudo apt install rclone`
#      or `curl https://rclone.org/install.sh | sudo bash`)
#   2. Run: rclone config
#      - "n" for new remote, name it "dag-google" (or change REMOTE_NAME below)
#      - type: drive (Google Drive) — NOT Google Cloud Storage
#      - client_id / client_secret: from an OAuth Client ID (Desktop app) you
#        create in Google Cloud Console — NOT a service_account_file, that's
#        for server-to-server auth against its own storage, not your Drive
#      - scope: 1 (full access)
#      - leave root_folder_id and service_account_file blank
#      - "n" for advanced config
#      - "n" for auto-config on a headless box — open the printed URL on any
#        device with a browser, authorize, paste the code back
#      - "n" for team drive
#      - Make sure the Drive API is enabled for your Cloud project (rclone
#        will print a console.developers.google.com link with a 403 if not)
#   3. Confirm it worked: rclone lsd dag-google:
#
# Usage:
#   ./upload_transcripts_to_drive.sh [--dry-run] [exclude-session-id]
#
# The optional exclude-session-id argument skips one session's own transcript
# and its subagents/ subdirectory (e.g. a session still live/incomplete right
# now) — pass the session UUID (matches the top-level .jsonl filename, sans
# extension). Omit it to upload everything found.

set -uo pipefail

REMOTE_NAME="dag-google"
DRIVE_FOLDER="rdf-datalog-transcripts"
SOURCE_DIR="$HOME/.claude/projects/-home-dag-rdf-datalog"

DRY_RUN=""
EXCLUDE_SESSION_ID=""
for arg in "$@"; do
    if [[ "$arg" == "--dry-run" ]]; then
        DRY_RUN="--dry-run"
        echo "DRY RUN — no files will actually be uploaded"
    else
        EXCLUDE_SESSION_ID="$arg"
    fi
done

if ! command -v rclone &> /dev/null; then
    echo "rclone not found. See the setup comment at the top of this script." >&2
    exit 1
fi

if ! rclone lsd "${REMOTE_NAME}:" &> /dev/null; then
    echo "rclone remote '${REMOTE_NAME}:' isn't configured or isn't reachable." >&2
    echo "Run 'rclone config' first (see setup comment at top of this script)." >&2
    exit 1
fi

echo "Creating/confirming Drive folder: ${DRIVE_FOLDER}"
rclone mkdir "${REMOTE_NAME}:${DRIVE_FOLDER}"

echo "Scanning ${SOURCE_DIR} for transcripts..."
count=0
skipped=0
failed=0
failed_files=()

# -type f skips symlinks: Claude Code's crash-recovery sometimes forks a
# session into a new ID whose subagents/ dir symlinks back at the original
# session's files. The symlink target is a real file, found independently by
# this same `find` under its own origin session directory, so following the
# symlink here would just be a duplicate upload of the same content under two
# names — and rclone's local backend errors on some of these anyway (a known
# quirk: "readdirent: not a directory"). Skipping is both safe and correct.
while IFS= read -r -d '' f; do
    base="$(basename "$f")"
    if [[ -n "$EXCLUDE_SESSION_ID" ]]; then
        if [[ "$base" == "${EXCLUDE_SESSION_ID}.jsonl" ]] || [[ "$f" == *"/${EXCLUDE_SESSION_ID}/"* ]]; then
            skipped=$((skipped + 1))
            continue
        fi
    fi
    echo "  uploading: $base"
    if rclone copyto ${DRY_RUN} "$f" "${REMOTE_NAME}:${DRIVE_FOLDER}/${base}"; then
        count=$((count + 1))
    else
        echo "  FAILED: $base (continuing with the rest)" >&2
        failed=$((failed + 1))
        failed_files+=("$f")
    fi
done < <(find "$SOURCE_DIR" -type f -name "*.jsonl" -print0)

echo ""
echo "Done. Uploaded: ${count}, skipped: ${skipped}, failed: ${failed}"
if [[ ${failed} -gt 0 ]]; then
    echo "Failed files:"
    printf '  %s\n' "${failed_files[@]}"
fi
echo "Folder: $(rclone lsjson "${REMOTE_NAME}:" | python3 -c "
import json,sys
for f in json.load(sys.stdin):
    if f['Name'] == '${DRIVE_FOLDER}' and f.get('IsDir'):
        print('https://drive.google.com/drive/folders/' + f['ID'])
        break
" 2>/dev/null || echo "(run 'rclone lsjson ${REMOTE_NAME}:' to find the folder ID/link)")"

if [[ ${failed} -gt 0 ]]; then
    exit 1
fi
