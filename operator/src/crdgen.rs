use kube::CustomResourceExt;

use keramik_operator::network::Network;

fn main() {
    print!("{}", serde_yaml::to_string(&Network::crd()).unwrap())
}
