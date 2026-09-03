#include <array>
#include <cstdint>
#include <fstream>
#include <iostream>

namespace {

template <typename T>
bool read_at(std::ifstream& input, const std::uint64_t offset, T& value) {
    input.seekg(static_cast<std::streamoff>(offset), std::ios::beg);
    input.read(reinterpret_cast<char*>(&value), sizeof(value));
    return input.good();
}

} // namespace

int main(const int argc, char** argv) {
    if (argc != 2) {
        std::cerr << "expected mouth-worker executable path\n";
        return 2;
    }
    std::ifstream input(argv[1], std::ios::binary);
    std::uint16_t dos_magic{};
    std::uint32_t pe_offset{};
    std::uint32_t pe_signature{};
    std::uint16_t optional_magic{};
    std::uint16_t subsystem{};
    if (!input || !read_at(input, 0U, dos_magic) || dos_magic != 0x5a4dU ||
        !read_at(input, 0x3cU, pe_offset) || !read_at(input, pe_offset, pe_signature) ||
        pe_signature != 0x00004550U || !read_at(input, pe_offset + 24U, optional_magic) ||
        (optional_magic != 0x10bU && optional_magic != 0x20bU) ||
        !read_at(input, pe_offset + 24U + 68U, subsystem)) {
        std::cerr << "mouth-worker is not a valid PE32/PE32+ image\n";
        return 1;
    }
    if (subsystem != 2U) {
        std::cerr << "mouth-worker must use IMAGE_SUBSYSTEM_WINDOWS_GUI (2), got "
                  << subsystem << '\n';
        return 1;
    }
    std::cout << "PASS: npc-mouth-worker.exe uses the hidden Windows GUI subsystem\n";
    return 0;
}
