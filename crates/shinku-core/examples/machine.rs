//! Affiche ce que le moteur voit de la machine. Outil de mise au point.

use shinku_core::platform::{Environment, KnownFolder, system};

fn main() {
    let machine = system();
    for folder in KnownFolder::ALL {
        println!("{folder:?} = {:?}", machine.known_folder(folder));
    }
    for volume in machine.volumes() {
        println!("{volume:?}");
    }
    println!("synchronisés = {:?}", machine.sync_roots());
    println!("corbeille = {:?}", machine.recycle_bin());
    println!("fils = {}", machine.parallelism());
}
