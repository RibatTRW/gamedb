use std::collections::HashMap;

pub struct Player {
    health: i32,
    name: String,
}

impl Player {
    pub fn new(health: i32) -> Player {
        Player { health, name: String::from("player") }
    }

    pub fn update(&mut self, dt: i32) {
        self.health += dt;
    }

    pub fn get_health(&self) -> i32 {
        self.health
    }
}

fn helper(x: i32) -> i32 {
    x + 1
}

fn main() {
    let mut p = Player::new(100);
    p.update(1);
    println!("{}", helper(p.get_health()));
}
