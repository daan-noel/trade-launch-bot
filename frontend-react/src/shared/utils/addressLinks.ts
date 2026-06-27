export type AddressKind = 'token' | 'account' | 'transaction';

export interface AddressExplorerLinks {
  solscan: string;
  gmgn?: string;
}

export function getAddressExplorerLinks(
  kind: AddressKind,
  address: string,
): AddressExplorerLinks {
  switch (kind) {
    case 'token':
      return {
        solscan: `https://solscan.io/token/${address}`,
        gmgn: `https://gmgn.ai/sol/token/${address}`,
      };
    case 'account':
      return {
        solscan: `https://solscan.io/account/${address}`,
        gmgn: `https://gmgn.ai/sol/address/${address}`,
      };
    case 'transaction':
      return {
        solscan: `https://solscan.io/tx/${address}`,
      };
  }
}
