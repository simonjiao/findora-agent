echo "fast_half_1" >> update.log
for ((i=1; i<=10; i+=1)); do
	cd ~/findora/platform && make debug_env; stt init; cd - || exit
	bash -x ./update_cluster.sh fast_half_1 >>update.log 2>&1
	cd ~/findora/platform && make stop_debug_env
	mv /tmp/findora/devnet /tmp/findora/devnet.fast_half_1."$i"
done

<<'COMMENT'
echo "fast_half" >> update.log
for ((i=1; i<=10; i+=1)); do
	cd ~/platform && make debug_env; stt init; cd - || exit
	bash -x ./update_cluster.sh fast_half >>update.log 2>&1
	mv /tmp/findora/devnet /data/findora/devnet.fast_half."$i"
done

echo "fast_two_third" >> update.log
for ((i=1; i<=10; i+=1)); do
	cd ~/platform && make debug_env; stt init; cd - || exit
	bash -x ./update_cluster.sh fast_two_third >>update.log 2>&1
	mv /tmp/findora/devnet /data/findora/devnet.fast_two_third."$i"
done

echo "swarm_reboot" >> update.log
for ((i=1; i<=10; i+=1)); do
	cd ~/platform && make debug_env; stt init; cd - || exit
	bash -x ./update_cluster.sh swarm_reboot >>update.log 2>&1
	mv /tmp/findora/devnet /data/findora/devnet.swarm_reboot."$i"
done

echo "seq test" >> update.log
for ((i=60; i<=1200; i+=30)); do
	cd ~/platform && make debug_env; stt init; cd - || exit
	bash -x ./update_cluster.sh seq "$i" >>update.log 2>&1
	mv /tmp/findora/devnet /data/findora/devnet.seq."$i"
done
COMMENT
