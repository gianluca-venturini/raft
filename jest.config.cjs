module.exports = {
    preset: 'ts-jest',
    testEnvironment: 'node',
    testMatch: ['**/*.test.ts'], // specify the test file pattern
    testTimeout: 10_000,
};