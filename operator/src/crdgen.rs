use keramik_operator::lgen::spec::LoadGenerator;
use keramik_operator::network::Network;
use keramik_operator::simulation::Simulation;
use kube::CustomResourceExt;

fn main() {
    print!("{}", serde_yaml::to_string(&Network::crd()).unwrap());
    println!("---");
    print!("{}", serde_yaml::to_string(&Simulation::crd()).unwrap());
    println!("---");
    print!("{}", serde_yaml::to_string(&LoadGenerator::crd()).unwrap());
}
