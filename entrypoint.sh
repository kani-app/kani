#!/bin/sh

# Started as root so a freshly host-created bind mount can be chowned, and
# non-recursively so a restart with a large /library stays fast. An
# operator-pinned non-root user (e.g. runAsNonRoot) has nothing to fix.
if [ "$(id -u)" = "0" ]; then
    chown kani:kani /data /library
    # No --reset-env: that would wipe KANI_* / RUST_LOG and everything else
    # docker-compose's `environment:` block or `ENV` set, which kani-web needs.
    RUN_AS="setpriv --reuid=kani --regid=kani --clear-groups"
else
    RUN_AS=""
fi

while true; do
    if $RUN_AS /app/kani-web "$@"; then
        EXIT_CODE=0
    else
        EXIT_CODE=$?
    fi

    if [ "$EXIT_CODE" -eq 42 ]; then
        echo "[entrypoint] Restart requested (exit code 42), restarting in 1s..."
        sleep 1
    else
        exit "$EXIT_CODE"
    fi
done
