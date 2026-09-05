#include "npc/mouth_worker/review_cues.hpp"
#include <iostream>
#include <sstream>

int main() {
    using namespace npc::mouth;
    const std::string hash(64, 'a');
    const std::string header = "npc-mouth-cues-v1 24000 2400 " + hash + "\n";
    int failures = 0;
    const auto expect = [&](bool okay, const char* message) {
        if (!okay) { std::cerr << message << '\n'; ++failures; }
    };
    std::istringstream valid(header + "0 720 0\n720 1440 1\n1440 2400 8\n");
    const auto cues = read_review_cues(valid, 24000, 2400, hash);
    expect(review_viseme_at(cues, 719) == Viseme::silence, "pre-boundary silence");
    expect(review_viseme_at(cues, 720) == Viseme::bilabial, "exact bilabial onset");
    expect(review_viseme_at(cues, 1439) == Viseme::bilabial, "bilabial interval retained");
    expect(review_viseme_at(cues, 1440) == Viseme::rounded, "rounded onset");
    expect(review_viseme_at(cues, 2400) == Viseme::silence, "no cue beyond audio");
    for (const auto* invalid : {"0 700 0\n701 2400 1", "0 700 0\n699 2400 1",
                              "0 2401 0", "0 2400 99", "0 0 1", "0 2000 0",
                              "0 2400 0\ntrailing", "0 2400 -1"}) {
        bool rejected = false;
        try { std::istringstream input(header + invalid); (void)read_review_cues(input, 24000, 2400, hash); }
        catch (const std::runtime_error&) { rejected = true; }
        expect(rejected, "malformed or misbound cues must be rejected");
    }
    bool rejected = false;
    try { std::istringstream input(header + "0 2400 0"); (void)read_review_cues(input, 24000, 2400, std::string(64, 'b')); }
    catch (const std::runtime_error&) { rejected = true; }
    expect(rejected, "same-duration wrong audio must be rejected");
    return failures == 0 ? 0 : 1;
}
