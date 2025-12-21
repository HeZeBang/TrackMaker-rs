sudo ip netns add ns_tun

sudo ip link set tun1 netns ns_tun

sudo ip netns exec ns_tun ip addr add 192.168.1.2/24 dev tun1
sudo ip netns exec ns_tun ip link set tun1 up
sudo ip netns exec ns_tun ip route add default via 192.168.1.1 dev tun1

sudo ip netns exec ns_tun sh -c 'echo "nameserver 10.15.44.11" > /etc/resolv.conf'

sudo ip netns exec ns_tun elinks https://example.com
