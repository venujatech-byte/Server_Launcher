set -u

clean() {
    printf '%s' "$1" | tr -d '\011\012\015' | sed 's/%/%25/g'
}

field() {
    printf 'SLD/1\tfield\t%s\t%s\t%s\n' "$1" "$2" "$(clean "$3")"
}

listener() {
    printf 'SLD/1\tlistener\t%s\t%s\t%s\t%s\t%s\t%s\n' \
        "$1" "$(clean "$2")" "$3" "$(clean "$4")" "$5" "$(clean "$6")"
}

display() {
    printf 'SLD/1\tdisplay\tvnc\t%s\t%s\t%s\n' \
        "$(clean "$1")" "$2" "$(clean "$3")"
}

printf '%s\n' '# Listener snapshot (read-only)'
if command -v ss >/dev/null 2>&1; then
    ss -ltnp 2>&1 | awk 'NR <= 64 && /:3389([^0-9]|$)/ { print "# RDP " $0; next } NR <= 64 && /:59(0[0-9]|10)([^0-9]|$)/ { print "# VNC " $0 }'
else
    printf '%s\n' '# ss unavailable'
fi
printf 'SLD/1\tbegin\n'

os_field() {
    sed -n "s/^$1=//p" /etc/os-release 2>/dev/null | sed -n '1p' | sed 's/^"//; s/"$//'
}

field host os_id "$(os_field ID)"
field host os_like "$(os_field ID_LIKE)"
field host os_version "$(os_field VERSION_ID)"
field host os_pretty_name "$(os_field PRETTY_NAME)"
if [ -d /run/systemd/system ]; then
    field host init systemd
elif [ -x /sbin/init ] || [ -x /usr/sbin/init ]; then
    field host init other
else
    field host init unknown
fi
field host user "$(id -un 2>/dev/null || printf '%s' unknown)"
uid=$(id -u 2>/dev/null || true)
case "$uid" in ''|*[!0-9]*) ;; *) field host uid "$uid" ;; esac
if command -v sudo >/dev/null 2>&1; then
    field host sudo_available 1
    if sudo -n true >/dev/null 2>&1; then field host passwordless_sudo 1; else field host passwordless_sudo 0; fi
else
    field host sudo_available 0
    field host passwordless_sudo 0
fi
if command -v apt-get >/dev/null 2>&1; then
    field host package_manager apt
elif command -v dnf >/dev/null 2>&1; then
    field host package_manager dnf
elif command -v yum >/dev/null 2>&1; then
    field host package_manager yum
elif command -v pacman >/dev/null 2>&1; then
    field host package_manager pacman
elif command -v zypper >/dev/null 2>&1; then
    field host package_manager zypper
else
    field host package_manager unknown
fi

if command -v dpkg-query >/dev/null 2>&1; then
    rdp_version=$(dpkg-query -W -f='${Version}' xrdp 2>/dev/null || true)
elif command -v rpm >/dev/null 2>&1; then
    rdp_version=$(rpm -q --qf '%{VERSION}-%{RELEASE}' xrdp 2>/dev/null || true)
else
    rdp_version=
fi
if command -v xrdp >/dev/null 2>&1 || [ -n "$rdp_version" ]; then field rdp installed 1; else field rdp installed 0; fi
field rdp version "$rdp_version"
if [ -d /run/systemd/system ] && command -v systemctl >/dev/null 2>&1; then
    if systemctl is-active --quiet xrdp.service && systemctl is-active --quiet xrdp-sesman.service; then field rdp unit_active 1; else field rdp unit_active 0; fi
    if systemctl is-enabled --quiet xrdp.service && systemctl is-enabled --quiet xrdp-sesman.service; then field rdp unit_enabled 1; else field rdp unit_enabled 0; fi
    if systemctl is-failed --quiet xrdp.service || systemctl is-failed --quiet xrdp-sesman.service; then
        field rdp unit_failed 1
        if command -v journalctl >/dev/null 2>&1; then journalctl -u xrdp.service -u xrdp-sesman.service -n 20 --no-pager 2>/dev/null | sed 's/^/# RDP log /'; fi
    else
        field rdp unit_failed 0
    fi
fi
if [ -r /etc/xrdp/xrdp.ini ]; then
    configured_port=$(awk 'BEGIN { globals=0 } /^\[globals\]/ { globals=1; next } /^\[/ { globals=0 } globals && /^[[:space:]]*port[[:space:]]*=/ { sub(/^[^=]*=[[:space:]]*/, ""); if ($0 ~ /^[0-9]+$/) { print; exit } }' /etc/xrdp/xrdp.ini)
    case "$configured_port" in
        ''|*[!0-9]*) ;;
        *) if [ "$configured_port" -gt 0 ] && [ "$configured_port" -le 65535 ]; then field rdp configured_port "$configured_port"; fi ;;
    esac
fi

vnc_binary=0
for binary in Xtigervnc tigervncserver vncserver Xvnc; do
    if command -v "$binary" >/dev/null 2>&1; then vnc_binary=1; break; fi
done
if command -v dpkg-query >/dev/null 2>&1; then
    vnc_version=$(dpkg-query -W -f='${Version}' tigervnc-standalone-server 2>/dev/null || dpkg-query -W -f='${Version}' tigervnc-server 2>/dev/null || true)
elif command -v rpm >/dev/null 2>&1; then
    vnc_version=$(rpm -q --qf '%{VERSION}-%{RELEASE}' tigervnc-server 2>/dev/null || true)
else
    vnc_version=
fi
if [ "$vnc_binary" -eq 1 ] || [ -n "$vnc_version" ]; then field vnc installed 1; else field vnc installed 0; fi
field vnc version "$vnc_version"

if [ -d /run/systemd/system ] && command -v systemctl >/dev/null 2>&1; then
    vnc_units=$(systemctl list-units --all --type=service --no-legend 2>/dev/null | awk 'tolower($1) ~ /(vnc|tigervnc)/ { print $1 }')
    if [ -n "$vnc_units" ]; then
        vnc_unit_found=0; vnc_unit_active=0; vnc_unit_failed=0; vnc_unit_enabled=0
        for unit in $vnc_units; do
            case "$unit" in *[!a-zA-Z0-9_.:@-]*) continue ;; esac
            vnc_unit_found=1
            systemctl is-active --quiet "$unit" && vnc_unit_active=1
            systemctl is-failed --quiet "$unit" && vnc_unit_failed=1
            systemctl is-enabled --quiet "$unit" && vnc_unit_enabled=1
        done
        if [ "$vnc_unit_found" -eq 1 ]; then
            field vnc unit_active "$vnc_unit_active"
            field vnc unit_failed "$vnc_unit_failed"
            field vnc unit_enabled "$vnc_unit_enabled"
            if [ "$vnc_unit_failed" -eq 1 ] && command -v journalctl >/dev/null 2>&1; then
                for unit in $vnc_units; do
                    case "$unit" in *[!a-zA-Z0-9_.:@-]*) continue ;; esac
                    journalctl -u "$unit" -n 12 --no-pager 2>/dev/null | sed 's/^/# VNC log /'
                done
            fi
        fi
    fi
fi

if command -v pgrep >/dev/null 2>&1 && command -v ps >/dev/null 2>&1; then
    vnc_found=0
    for binary in Xtigervnc Xvnc; do
        for pid in $(pgrep -x "$binary" 2>/dev/null || true); do
            args=$(ps -p "$pid" -o args= 2>/dev/null || true)
            user=$(ps -p "$pid" -o user= 2>/dev/null | sed 's/^[[:space:]]*//;s/[[:space:]]*$//')
            display_number=$(printf '%s\n' "$args" | awk '{ for (i=1; i<=NF; i++) if ($i ~ /^:[0-9]+$/) { sub(/^:/, "", $i); print $i; exit } }')
            [ -n "$display_number" ] || continue
            case "$display_number" in ''|*[!0-9]*) continue ;; esac
            [ "$display_number" -le 59535 ] || continue
            display_port=$((5900 + display_number))
            candidate=$(printf '%s\n' "$args" | awk '{ for (i=1; i<NF; i++) if ($i == "-rfbport" && $(i+1) ~ /^[0-9]+$/) { print $(i+1); exit } }')
            case "$candidate" in ''|*[!0-9]*) ;; *) display_port=$candidate ;; esac
            [ "$display_port" -gt 0 ] && [ "$display_port" -le 65535 ] || continue
            display ":$display_number" "$display_port" "$user"
            vnc_found=1
        done
    done
    if [ "$vnc_found" -eq 1 ]; then field vnc manual_process 1; else field vnc manual_process 0; fi
fi

for session_dir in /usr/share/xsessions /usr/share/wayland-sessions; do
    for session in "$session_dir"/*.desktop; do
        [ -f "$session" ] || continue
        session_name=${session##*/}
        field host desktop_session "${session_name%.desktop}"
    done
done

if command -v ufw >/dev/null 2>&1; then
    field host firewall_hint "ufw: $(ufw status 2>&1 | sed -n '1p')"
fi
if command -v firewall-cmd >/dev/null 2>&1; then
    field host firewall_hint "firewalld: $(firewall-cmd --state 2>&1 | sed -n '1p')"
fi

if [ -r /proc/meminfo ]; then
    memory=$(awk '/^MemAvailable:/ { print int($2 / 1024); exit }' /proc/meminfo)
    case "$memory" in ''|*[!0-9]*) ;; *) field host available_memory_mb "$memory" ;; esac
fi
disk=$(df -Pm "${HOME:-/}" 2>/dev/null | awk 'NR == 2 { print $4; exit }')
case "$disk" in ''|*[!0-9]*) ;; *) field host free_disk_mb "$disk" ;; esac

if command -v ss >/dev/null 2>&1; then
    ss -H -ltnp 2>/dev/null | awk '
        {
            endpoint = $4
            count = split(endpoint, parts, ":")
            port = parts[count] + 0
            if (port != 3389 && (port < 5900 || port > 5910)) next
            address = substr(endpoint, 1, length(endpoint) - length(parts[count]) - 1)
            protocol = port == 3389 ? "rdp" : "vnc"
            process = "-"; pid = "-"; owner = "-"
            for (i = 1; i <= NF; i++) if ($i ~ /users:\(\(/) {
                process = $i
                sub(/^.*users:\(\("/, "", process)
                sub(/".*$/, "", process)
                if ($i ~ /pid=[0-9]+/) { pid = $i; sub(/^.*pid=/, "", pid); sub(/[^0-9].*$/, "", pid) }
            }
            gsub(/%/, "%25", address)
            gsub(/%/, "%25", process)
            printf "SLD/1\tlistener\t%s\t%s\t%d\t%s\t%s\t%s\n", protocol, address, port, process, pid, owner
        }
    '
elif command -v netstat >/dev/null 2>&1; then
    netstat -ltnp 2>/dev/null | awk '
        $1 ~ /^tcp/ && $6 == "LISTEN" {
            endpoint = $4
            count = split(endpoint, parts, ":")
            port = parts[count] + 0
            if (port != 3389 && (port < 5900 || port > 5910)) next
            address = substr(endpoint, 1, length(endpoint) - length(parts[count]) - 1)
            protocol = port == 3389 ? "rdp" : "vnc"
            process = "-"; pid = "-"; owner = "-"
            if ($7 != "-" && index($7, "/") > 0) {
                split($7, owner_parts, "/")
                pid = owner_parts[1]
                process = owner_parts[2]
            }
            gsub(/%/, "%25", address)
            gsub(/%/, "%25", process)
            printf "SLD/1\tlistener\t%s\t%s\t%d\t%s\t%s\t%s\n", protocol, address, port, process, pid, owner
        }
    '
else
    awk '
        function hexbyte(value, i, digit, number) {
            value = toupper(value); number = 0
            for (i = 1; i <= length(value); i++) {
                digit = index("0123456789ABCDEF", substr(value, i, 1)) - 1
                if (digit < 0) return 0
                number = number * 16 + digit
            }
            return number
        }
        FNR > 1 && $4 == "0A" {
            split($2, endpoint, ":")
            port = hexbyte(endpoint[2])
            if (port != 3389 && (port < 5900 || port > 5910)) next
            if (FILENAME ~ /tcp6$/) {
                address = endpoint[1] ~ /^0+$/ ? "::" : "unknown"
            } else {
                hex = endpoint[1]
                address = sprintf("%d.%d.%d.%d", hexbyte(substr(hex, 7, 2)), hexbyte(substr(hex, 5, 2)), hexbyte(substr(hex, 3, 2)), hexbyte(substr(hex, 1, 2)))
            }
            protocol = port == 3389 ? "rdp" : "vnc"
            printf "SLD/1\tlistener\t%s\t%s\t%d\t-\t-\t-\n", protocol, address, port
        }
    ' /proc/net/tcp /proc/net/tcp6 2>/dev/null
fi
printf 'SLD/1\tend\n'
