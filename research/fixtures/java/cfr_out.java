// CFR / JD-GUI decompiler output
package acme.core;

import java.util.List;

public class Player extends Entity implements Tickable {
    private int health;
    private String name;

    public Player(int health) {
        this.health = health;
        this.name = "player";
    }

    @Override
    public void update(int dt) throws IllegalStateException {
        this.health += dt;
        System.out.println("tick");
    }

    public int getHealth() {
        return this.health;
    }

    public static <T> List<T> wrap(List<T> input) {
        return input;
    }

    public final synchronized void reset() {
        this.health = 0;
    }

    public boolean isHealthy() { return this.health > 0; }
}

enum State {
    IDLE, RUNNING(3), DEAD;

    private final int speed;

    State() { this.speed = 0; }

    State(int speed) { this.speed = speed; }

    public int speed() { return this.speed; }
}

record Point(int x, int y) {
    public Point {
        if (x < 0) {
            throw new IllegalArgumentException("x");
        }
    }

    public int sum() { return x + y; }
}

class Outer {
    private int depth;

    class Inner {
        public void dive() { }
    }
}
