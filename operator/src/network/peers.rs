use std::collections::BTreeMap;

use keramik_common::peer_info::PeerInfo;

pub fn peer_config_map_data(peers: &[PeerInfo]) -> BTreeMap<String, String> {
    BTreeMap::from_iter(vec![(
        "peers.json".to_owned(),
        serde_json::to_string(peers).unwrap(),
    )])
}
