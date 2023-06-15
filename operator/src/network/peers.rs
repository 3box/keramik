use std::collections::BTreeMap;

use keramik_common::peer_info::PeerInfo;

pub const PEERS_MAP_KEY: &str = "peers.json";

pub fn peer_config_map_data(peers: &[PeerInfo]) -> BTreeMap<String, String> {
    BTreeMap::from_iter(vec![(
        PEERS_MAP_KEY.to_owned(),
        serde_json::to_string(peers).unwrap(),
    )])
}
