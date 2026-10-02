package acme.core

class Player(private var health: Int) {
    fun update(dt: Int) {
        health += dt
    }

    fun getHealth(): Int {
        return health
    }

    fun isDead(): Boolean = health <= 0
}

fun createPlayer(health: Int): Player {
    return Player(health)
}
