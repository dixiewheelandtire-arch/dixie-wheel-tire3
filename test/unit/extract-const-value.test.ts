import { extractExportedConstValue } from 'next/src/build/analysis/extract-const-value'
import { parse } from '@swc/core'

async function parseCode(code: string) {
  return parse(code, { syntax: 'typescript' })
}

describe('extractExportedConstValue', () => {
  it('extracts string literal', async () => {
    const ast = await parseCode(`export const foo = "bar"`)
    expect(extractExportedConstValue(ast, 'foo')).toBe('bar')
  })

  it('extracts template literal', async () => {
    const ast = await parseCode('export const foo = `/api/test`')
    expect(extractExportedConstValue(ast, 'foo')).toBe('/api/test')
  })

  it('extracts String.raw tagged template', async () => {
    const ast = await parseCode(
      'export const foo = String.raw`/api/:path*`'
    )
    expect(extractExportedConstValue(ast, 'foo')).toBe('/api/:path*')
  })

  it('extracts String.raw with backslashes preserved', async () => {
    const ast = await parseCode(
      'export const foo = String.raw`\\n\\t`'
    )
    expect(extractExportedConstValue(ast, 'foo')).toBe('\\n\\t')
  })

  it('extracts array with String.raw elements', async () => {
    const ast = await parseCode(
      'export const matcher = [String.raw`/api/:path*`, "/about"]'
    )
    expect(extractExportedConstValue(ast, 'matcher')).toEqual([
      '/api/:path*',
      '/about',
    ])
  })

  it('throws on String.raw with expressions', async () => {
    const ast = await parseCode(
      'export const foo = String.raw`/api/${path}`'
    )
    expect(() => extractExportedConstValue(ast, 'foo')).toThrow(
      'Unsupported String.raw template literal with expressions'
    )
  })

  it('throws on non-String.raw tagged template', async () => {
    const ast = await parseCode('export const foo = html`<div>test</div>`')
    expect(() => extractExportedConstValue(ast, 'foo')).toThrow(
      'Unsupported tagged template expression'
    )
  })
})
