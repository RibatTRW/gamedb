struct Multi {
    int a, b, c;
    char name[32];
    union {
        int i;
        float f;
    } value;
    struct { int nested; } inner;
};
typedef struct {
    int only;
} OnlyAnon;
