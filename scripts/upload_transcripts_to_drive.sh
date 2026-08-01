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
#      - "n" for new remote, name it "gdrive" (or change REMOTE_NAME below)
#      - type: drive (Google Drive)
#      - client_id / client_secret: leave blank to use rclone's own
#      - scope: 1 (full access) or 2 (read/write to files rclone created, if you prefer tighter scope)
#      - leave root_folder_id blank
#      - "n" for advanced config
#      - "y" to auto-authenticate via browser (or "n" + follow the printed URL if headless)
#      - "n" for team drive
#   3. Confirm it worked: rclone lsd gdrive:
#
# Usage:
#   ./upload_transcripts_to_drive.sh [--dry-run]

set -euo pipefail

REMOTE_NAME="gdrive"
DRIVE_FOLDER="rdf-datalog-transcripts"
SOURCE_DIR="$HOME/.claude/projects/-home-dag-rdf-datalog"

# Exclude the current, still-in-progress session — upload it later once it's
# actually finished. Adjust or remove this if you want to include it anyway.
EXCLUDE_SESSION_ID="c6466b42-5268-422e-892e-2d47f77887be"

DRY_RUN=""
if [[ "${1:-}" == "--dry-run" ]]; then
    DRY_RUN="--dry-run"
    echo "DRY RUN — no files will actually be uploaded"
fi

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
while IFS= read -r -d '' f; do
    base="$(basename "$f")"
    if [[ "$base" == "${EXCLUDE_SESSION_ID}.jsonl" ]] || [[ "$f" == *"/${EXCLUDE_SESSION_ID}/"* ]]; then
        skipped=$((skipped + 1))
        continue
    fi
    echo "  uploading: $base"
    rclone copyto ${DRY_RUN} "$f" "${REMOTE_NAME}:${DRIVE_FOLDER}/${base}"
    count=$((count + 1))
done < <(find "$SOURCE_DIR" -name "*.jsonl" -print0)

echo ""
echo "Done. Uploaded: ${count}, skipped (current session): ${skipped}"
echo "Folder: $(rclone lsjson "${REMOTE_NAME}:" | python3 -c "
import json,sys
for f in json.load(sys.stdin):
    if f['Name'] == '${DRIVE_FOLDER}' and f.get('IsDir'):
        print('https://drive.google.com/drive/folders/' + f['ID'])
        break
" 2>/dev/null || echo "(run 'rclone lsjson ${REMOTE_NAME}:' to find the folder ID/link)")"
