package acme.core

class Player(private var health: Int) {
  def update(dt: Int): Unit = {
    health += dt
  }

  def getHealth(): Int = {
    return health
  }

  def isDead: Boolean = health <= 0
}

object Player {
  def create(health: Int): Player = {
    new Player(health)
  }
}
