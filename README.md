# TrackMaker-rs
A high-performance audio-based information transmission tool, written in Rust

## Note on MacOS

To fully utilize JACK on macOS, you may need to install additional components such as `jack` via Homebrew:

```bash
brew install jack
```

Normally, the JACK server will start in 44100Hz with a buffer size of 512 samples. To change this settings, start the JACK server by:

```bash
jackd -d coreaudio -r 48000 -p 256
```

If you're launching this program on MacOS with homebrew, link the dynamic libraries by:

```bash
export DYLD_LIBRARY_PATH="$HOME/homebrew/lib:$DYLD_LIBRARY_PATH"
```

Additionally, we found the provided device will get much more noise when output volume is over 30%, se we recommend playback `0.29` or `-17dB` and record `0.64` or `16dB` to get the bset result.

## Note for Linux Pipewire

Pipewire contins its default jack implementation, to dajust settings, use:

```bash
pw-metadata -n settings 0 clock.force-rate 48000
pw-metadata -n settings 0 clock.force-quantum 128
```

Sometimes Pipeware will oversample if you choose the volume that is too large, so my best trail is to set OUTPUT to about `31%` / `-30.63dB` and record to `153%` / `11.0dB`.

For best performance with pipewire jack server, use the following command:

```bash
PIPEWIRE_QUANTUM=256/48000 pw-jack ./target/release/trackmaker-rs
```

jackd

```bash
jackd -dalsa -r48000 -p128 -Xraw -D -Chw:Device -Phw:Device
```

## Disable ECHO

```bash
sysctl -w net.ipv4.icmp_echo_ignore_all=1
```

## Project 3

```bash
export WLAN_IF="wlan0"
export ETH_IF="wlp0s20f3"
export WLAN_IP=$(ip -4 addr show dev $WLAN_IF | awk '/inet /{print $2}' | cut -d/ -f1)
export WLAN_MAC=$(ip link show dev $WLAN_IF | awk '/link\/ether/{print $2}')
export ETH_IP=$(ip -4 addr show dev $ETH_IF | awk '/inet /{print $2}' | cut -d/ -f1)
export ETH_MAC=$(ip link show dev $ETH_IF | awk '/link\/ether/{print $2}')
export GTW_IP=$(ip route show default dev $ETH_IF | awk '{print $3}')
export GTW_MAC=$(ip neigh show $GTW_IP dev $ETH_IF | awk '{print $3}')
echo "Device: Eth/Gtw - $ETH_IF, Hotspot - $WLAN_IF\nHotspot:\t$WLAN_IP\t($WLAN_MAC)\nEthernet:\t$ETH_IP\t($ETH_MAC)\nGateway:\t$GTW_IP\t($GTW_MAC)"

PIPEWIRE_QUANTUM=128/48000 pw-jack ./target/debug/trackmaker-rs router --wifi-interface $WLAN_IF --wifi-ip $WLAN_IP --wifi-mac $WLAN_MAC --node3-ip 10.42.0.2 --gateway-ip $GTW_IP --gateway-mac $GTW_MAC --gateway-interface $ETH_IF --eth-ip $ETH_IP --eth-mac $ETH_MAC --tun-ip 10.0.0.1 --tun-name tun0
```

---

```bash
PIPEWIRE_QUANTUM=128/48000 pw-jack ./target/debug/trackmaker-rs router --wifi-interface wlan0 --wifi-ip 10.42.0.1 --wifi-mac 6c:1f:f7:7b:d2:02 --node3-ip 10.42.0.2 --gateway-ip 10.20.100.1 --gateway-mac 00:00:5e:00:01:01 --gateway-interface wlp0s20f3 --eth-ip 10.20.239.6 --eth-mac 9c:29:76:0c:49:00 --tun-ip 10.0.0.1 --tun-name tun0
```

- `wifi-interface`: Device Name for WLAN Hotspot (Not for ethernet)
- `wifi-ip`, `wifi-mac`
- `node3-ip`: IP Address for Node3
- `node3-mac`(Optional): Mac for Node3