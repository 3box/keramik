use kube::CustomResourceExt;

use keramik_operator::network::Network;
use keramik_operator::simulation::Simulation;

fn main() {
    print!("{}", serde_yaml::to_string(&Network::crd()).unwrap());
    print!("---");
    print!("{}", serde_yaml::to_string(&Simulation::crd()).unwrap());
}
