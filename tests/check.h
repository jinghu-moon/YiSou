#pragma once

#include <stdexcept>

#define CHECK(...) do { if (!(__VA_ARGS__)) throw std::runtime_error(#__VA_ARGS__); } while (false)
