namespace Acme {
    public record Person(string Name, int Age);
    public class Widget(int size) {
        private int size = size;
        public int this[int i] { get { return i; } }
        public static Widget operator +(Widget a, Widget b) { return a; }
        ~Widget() { }
        public void Run() { }
    }
    public interface IThing {
        void Do();
        void Go() { }
    }
}
