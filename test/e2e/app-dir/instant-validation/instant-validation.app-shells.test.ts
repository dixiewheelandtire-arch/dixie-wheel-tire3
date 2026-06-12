// This is not ideal, but the test suite is huge and duplicating it
// would make it very annoying to keep up to date.
process.env.NEXT_TEST_ENABLE_APP_SHELLS = '1'
require('./instant-validation.test')
