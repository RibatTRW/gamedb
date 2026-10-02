import { EventEmitter } from "events";

export class Player extends EventEmitter {
  private health: number;

  constructor(health: number) {
    super();
    this.health = health;
  }

  update(dt: number): void {
    this.health += dt;
  }

  getHealth(): number {
    return this.health;
  }
}

export function createPlayer(health: number): Player {
  return new Player(health);
}

export const fastPlayer = (): Player => {
  const p = new Player(200);
  return p;
};

const anonymous = function (x: number) {
  return x + 1;
};
