package acme;
import java.util.function.Supplier;
class Anon {
    void go() {
        Runnable r = new Runnable() {
            @Override public void run() { }
        };
        r.run();
    }
}
interface Api {
    void onDone(int code);
    default void onError() { }
}
