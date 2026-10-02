// IDA Hex-Rays C++ output
#include <memory>

namespace Game {
namespace Core {

class Player {
public:
    Player();
    ~Player();
    void Update(int dt);
    int GetHealth() const;
    static Player *Create();
private:
    int health_;
    std::string name_;
};

void Player::Update(int dt) {
    health_ += dt;
}

int Player::GetHealth() const {
    return health_;
}

Player *Player::Create() {
    return new Player();
}

std::shared_ptr<Player> MakePlayer() {
    return std::make_shared<Player>();
}

} // namespace Core
} // namespace Game

int main(int argc, char **argv) {
    Game::Core::Player p;
    p.Update(1);
    return 0;
}
