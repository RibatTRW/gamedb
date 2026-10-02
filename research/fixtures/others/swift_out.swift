import Foundation

public class Player {
    private var health: Int

    public init(health: Int) {
        self.health = health
    }

    public func update(dt: Int) {
        health += dt
    }

    public func getHealth() -> Int {
        return health
    }
}

public func createPlayer(health: Int) -> Player {
    return Player(health: health)
}
