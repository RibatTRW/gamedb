typedef struct { int a; int b; } Anon;
typedef int MyInt;
struct Named { int x; int y; };
struct Outer { struct Inner { int z; } in; int w; };
void (*handler)(int);
int g1, g2, g3;
struct Flags { unsigned a : 3; unsigned b : 5; };
enum Color { RED, GREEN, BLUE };
static void helper(void) { }
