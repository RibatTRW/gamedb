/* Ghidra-style decompiler output */
typedef struct SavedEntity {
    int id;
    char name[32];
    float x;
    float y;
} SavedEntity;

struct WorldState {
    int tick;
    SavedEntity *entities;
};

undefined4 FUN_00401234(int param_1, char *param_2)
{
  size_t sVar1;
  sVar1 = strlen(param_2);
  return (undefined4)(sVar1 + param_1);
}

void FUN_00401299(SavedEntity *e)
{
  printf("entity %d\n", e->id);
  e->tick = 0;
}

static int helper_add(int a, int b)
{
  return a + b;
}
