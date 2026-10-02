// ILSpy decompiler output
using System;
using System.Collections.Generic;

namespace Acme.Core
{
    [Serializable]
    public sealed class Player : Entity, IDamageable
    {
        private int health;
        private string name;

        public int Health { get; set; }
        public string Name => name;

        public event EventHandler Died;

        public Player(int health)
        {
            this.health = health;
            this.name = "player";
        }

        ~Player()
        {
            this.Died = null;
        }

        public void Update(int dt)
        {
            this.health += dt;
            Console.WriteLine("tick");
        }

        public int TakeDamage(int amount) => this.health -= amount;

        public static Player Create() => new Player(100);

        public List<T> Wrap<T>(List<T> input) where T : class
        {
            return input;
        }
    }

    public interface IDamageable
    {
        int TakeDamage(int amount);
    }
}
