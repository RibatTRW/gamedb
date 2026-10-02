class Vec {
public:
    Vec();
    ~Vec();
    bool operator==(const Vec& o) const;
    auto size() const -> int;
};
Vec::Vec() : x_(0) { }
Vec::~Vec() { }
bool Vec::operator==(const Vec& o) const { return true; }
auto Vec::size() const -> int { return 0; }
template <typename T> T clamp(T v, T lo, T hi) { return v; }
template <typename T> class Box { public: T value; T get() const; };
