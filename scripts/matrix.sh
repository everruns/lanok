#!/bin/sh
# Every client against every server, across all three languages.
#
# Nine combinations, each one a full exchange: handshake, forward request,
# progress notifications, and a reverse request the server sends back to the
# client. This is what "Python and TypeScript are first-class" means, checked
# rather than asserted. If a language could only serve, or only drive, a row or
# a column here would be missing.
#
# Usage: scripts/matrix.sh        (expects the binaries and the TS build present)
set -eu

CLIENTS='rust python typescript'
SERVERS='rust python typescript'

client_command() {
	case "$1" in
	rust) echo "cargo run -q -p echo-protocol --bin echo-client -- --server" ;;
	python) echo "python3 examples/echo-python/client.py" ;;
	typescript) echo "node examples/echo-typescript/client.mjs" ;;
	esac
}

server_command() {
	case "$1" in
	rust) echo "./target/debug/echo-server --async" ;;
	python) echo "python3 examples/echo-python/peer_server.py" ;;
	typescript) echo "node examples/echo-typescript/peerServer.mjs" ;;
	esac
}

failures=0
for client in $CLIENTS; do
	for server in $SERVERS; do
		printf '%-12s client -> %-12s server  ' "$client" "$server"
		if $(client_command "$client") $(server_command "$server") >/tmp/lanok-matrix.log 2>&1; then
			echo 'ok'
		else
			echo 'FAIL'
			sed 's/^/    /' /tmp/lanok-matrix.log
			failures=$((failures + 1))
		fi
	done
done

if [ "$failures" -gt 0 ]; then
	echo "$failures of 9 combinations failed"
	exit 1
fi
echo 'all 9 combinations passed'
